import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import { initConfig, readConfig } from './config.ts';
import { committedRows, createConversation, getConversation, toDisplay } from './conversations.ts';
import {
	CustomOpenaiError,
	checkCustomOpenai,
	customOpenai,
	customOpenaiStatus,
	removeCustomOpenai,
	saveCustomOpenai
} from './custom-openai.ts';
import {
	describeApiError,
	fetchContextWindow,
	listModels,
	quickReply,
	readableMessages,
	streamTurn,
	type Effort
} from './models.ts';
import { RUN_COMMAND_TOOL, runCommand } from './run-command.ts';
import { onLoopEnd, sendMessage } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

// --- a stand-in for a server that speaks OpenAI's API, like Ollama's or vLLM's ---

interface Seen {
	method: string;
	path: string;
	key: string | undefined;
	json: Record<string, unknown> | null;
}
type Answer = { status?: number; json?: unknown; events?: object[] };

let server: Server;
let baseUrl = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;
/** The key it wants, if any. */
let wantsKey: string | null = null;
let models: object[] = [];

beforeAll(async () => {
	server = createServer(async (req, res) => {
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk as Buffer);
		const body = Buffer.concat(chunks).toString('utf8');
		const request: Seen = {
			method: req.method ?? 'GET',
			path: req.url ?? '',
			key: req.headers.authorization?.replace(/^Bearer /, ''),
			json: body ? (JSON.parse(body) as Seen['json']) : null
		};
		seen.push(request);
		const reply: Answer =
			wantsKey && request.key !== wantsKey
				? { status: 401, json: { error: { message: 'Invalid API key' } } }
				: request.path === '/v1/models'
					? { json: { object: 'list', data: models } }
					: answer(request);
		if (reply.events) {
			res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream' });
			res.end(
				reply.events
					.map((e) => `event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`)
					.join('')
			);
		} else {
			res.writeHead(reply.status ?? 200, { 'content-type': 'application/json' });
			res.end(JSON.stringify(reply.json ?? {}));
		}
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	initConfig();
	vi.stubEnv('CUSTOM_OPENAI_BASE_URL', '');
	vi.stubEnv('CUSTOM_OPENAI_API_KEY', '');
	seen.length = 0;
	wantsKey = null;
	models = [
		{ id: 'qwen3:8b', object: 'model', created: 1, owned_by: 'library' },
		{ id: 'Qwen/Qwen3-32B', object: 'model', max_model_len: 40_960 }
	];
	answer = () => ({ status: 404, json: { error: { message: 'Not Found' } } });
});

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetAllMocks();
});

const usage = (input: number, output: number) => ({ input_tokens: input, output_tokens: output });

/**
 * A streamed response, item by item, as vLLM and LM Studio send one: a reasoning item's full
 * text (`reasoning_text`, where OpenAI gives a summary), then the message or the calls.
 */
function streamed(output: Record<string, unknown>[], used = usage(900, 30)): Answer {
	const events: object[] = [{ type: 'response.created', response: { status: 'in_progress' } }];
	output.forEach((item, index) => {
		events.push({ type: 'response.output_item.added', output_index: index, item });
		if (item.type === 'reasoning') {
			for (const part of item.content as { text: string }[]) {
				events.push({
					type: 'response.reasoning_text.delta',
					output_index: index,
					delta: part.text
				});
			}
		} else if (item.type === 'message') {
			for (const part of item.content as { text: string }[]) {
				events.push({ type: 'response.output_text.delta', output_index: index, delta: part.text });
			}
		}
		events.push({ type: 'response.output_item.done', output_index: index, item });
	});
	events.push({
		type: 'response.completed',
		response: { status: 'completed', output, usage: used }
	});
	return { events };
}

const thought = {
	id: 'rs_1',
	type: 'reasoning',
	summary: [],
	content: [{ type: 'reasoning_text', text: 'Anna wants the files.' }]
};
const listArgs = JSON.stringify({
	summary: 'Listing the files',
	icon: 'folder-open',
	command: 'ls'
});
const listCall = {
	id: 'fc_1',
	type: 'function_call',
	status: 'completed',
	call_id: 'call_1',
	name: 'run_command',
	arguments: listArgs
};
function said(text: string) {
	return {
		id: 'msg_1',
		type: 'message',
		role: 'assistant',
		status: 'completed',
		content: [{ type: 'output_text', text, annotations: [] }]
	};
}

/** A thinking model's title, its thoughts left in the text as servers without a parser do. */
const titled = (title: string): Answer => ({
	json: {
		status: 'completed',
		output: [said(`<think>\nA short title.\n</think>\n\n${title}`)],
		usage: usage(60, 4)
	}
});

function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

function customChat(model = 'qwen3:8b') {
	saveCustomOpenai(baseUrl, null);
	const { user, profile } = makeFamily();
	const preset = makePreset('On the server', model, 'custom-openai');
	return {
		user,
		profile,
		chat: createConversation({ profile, presetId: preset.id, userId: user.id })
	};
}

const RESPONSES = '/v1/responses';
const turns = () => seen.filter((r) => r.path === RESPONSES && r.json?.stream);

/** One call on the server's model, as the runner makes it. */
const turn = (model: string, effort: Effort = 'medium') =>
	streamTurn({
		provider: 'custom-openai',
		model,
		effort,
		system: 'Be brief.',
		tools: [],
		cacheTtl: '1h',
		cacheKey: 'conv-1',
		messages: [{ role: 'user', blocks: [{ type: 'text', text: 'Hi' }] }],
		signal: new AbortController().signal,
		onEvent: () => {}
	});

// --- tests ---

describe('the Custom OpenAI server', () => {
	it('is saved with its key, which never shows, and falls back to the environment', () => {
		expect(customOpenai()).toBeNull();
		expect(customOpenaiStatus()).toMatchObject({ url: null, source: null, hasKey: false });

		saveCustomOpenai('http://localhost:11434/v1/', 'sk-local-key-0000000042');
		expect(customOpenai()).toEqual({
			url: 'http://localhost:11434/v1',
			key: 'sk-local-key-0000000042'
		});
		const status = customOpenaiStatus();
		expect(status).toEqual({
			url: 'http://localhost:11434/v1',
			source: 'config',
			hasKey: true,
			hint: '0042',
			envSet: false
		});
		expect(JSON.stringify(status)).not.toContain('sk-local');

		// A form that never showed the key keeps it for the same address, not for another.
		saveCustomOpenai('http://localhost:11434/v1', undefined);
		expect(customOpenai()?.key).toBe('sk-local-key-0000000042');
		saveCustomOpenai('http://localhost:1234/v1', undefined);
		expect(customOpenai()).toEqual({ url: 'http://localhost:1234/v1', key: null });

		removeCustomOpenai();
		expect(readConfig().customOpenaiUrl).toBeUndefined();
		vi.stubEnv('CUSTOM_OPENAI_BASE_URL', 'http://gpu-box:8000/v1');
		vi.stubEnv('CUSTOM_OPENAI_API_KEY', 'token');
		expect(customOpenai()).toEqual({ url: 'http://gpu-box:8000/v1', key: 'token' });
		expect(customOpenaiStatus()).toMatchObject({ source: 'env', envSet: true, hint: null });
		// The environment's key is its server's: another one saved in btw doesn't get it.
		saveCustomOpenai('http://localhost:11434/v1', null);
		expect(customOpenai()).toEqual({ url: 'http://localhost:11434/v1', key: null });
		saveCustomOpenai('http://gpu-box:8000/v1/', null);
		expect(customOpenai()?.key).toBe('token');
		vi.stubEnv('CUSTOM_OPENAI_BASE_URL', '');
		saveCustomOpenai('http://localhost:11434/v1', null);
		expect(customOpenai()?.key).toBe('token');
	});

	it('is checked by asking it for its models', async () => {
		expect(await checkCustomOpenai(baseUrl, null)).toEqual({
			models: ['qwen3:8b', 'Qwen/Qwen3-32B'],
			warning: null
		});

		wantsKey = 'secret';
		await expect(checkCustomOpenai(baseUrl, null)).rejects.toMatchObject({
			reason: 'key',
			message: 'The server wants a key.'
		});
		await expect(checkCustomOpenai(baseUrl, 'wrong')).rejects.toMatchObject({ reason: 'key' });
		expect((await checkCustomOpenai(`${baseUrl}/`, 'secret')).models).toHaveLength(2);
		expect(seen.at(-1)).toMatchObject({ path: '/v1/models', key: 'secret' });

		models = [];
		wantsKey = null;
		expect(await checkCustomOpenai(baseUrl, null)).toEqual({
			models: [],
			warning: "It doesn't list any models yet."
		});
		const closed = `http://127.0.0.1:1/v1`;
		await expect(checkCustomOpenai(closed, null)).rejects.toMatchObject({
			reason: 'unreachable'
		});
		await expect(checkCustomOpenai('localhost:11434', null)).rejects.toBeInstanceOf(
			CustomOpenaiError
		);
	});

	it("lists its models, with a window where it says one, and checks a preset's", async () => {
		saveCustomOpenai(baseUrl, null);
		expect(await listModels('custom-openai')).toEqual([
			{ id: 'qwen3:8b', name: null, description: null, contextWindow: null },
			{ id: 'Qwen/Qwen3-32B', name: null, description: null, contextWindow: 40_960 }
		]);
		expect(await fetchContextWindow('custom-openai', 'Qwen/Qwen3-32B')).toBe(40_960);
		expect(await fetchContextWindow('custom-openai', 'qwen3:8b')).toBeNull();
		await expect(fetchContextWindow('custom-openai', 'llama3')).rejects.toThrow(
			'has no model "llama3". It serves qwen3:8b, Qwen/Qwen3-32B.'
		);
		// A server that lists none can't be asked: whatever it serves shows at the first reply.
		models = [];
		expect(await fetchContextWindow('custom-openai', 'llama3')).toBeNull();
	});

	it('says what went wrong in words, for a missing server, a key or one that is down', async () => {
		const noServer = await quickReply({
			provider: 'custom-openai',
			model: 'qwen3:8b',
			system: 'Be brief.',
			input: 'Hi',
			maxTokens: 10,
			timeoutMs: 5000
		}).catch((err: unknown) => err);
		expect(describeApiError(noServer)).toContain('No Custom OpenAI server yet');

		saveCustomOpenai(baseUrl, null);
		wantsKey = 'secret';
		const refused = await listModels('custom-openai').catch((err: unknown) => err);
		expect(describeApiError(refused)).toBe(
			`The Custom OpenAI server at ${baseUrl} didn't accept the key. An admin can change it under Models & keys in btw, or with \`btw key set custom-openai\`.`
		);

		saveCustomOpenai('http://127.0.0.1:1/v1', null);
		const down = await listModels('custom-openai').catch((err: unknown) => err);
		expect(describeApiError(down)).toMatch(
			/^Couldn't reach the Custom OpenAI server at http:\/\/127\.0\.0\.1:1\/v1 \(.+\)\. Is it running\?$/
		);

		saveCustomOpenai(baseUrl, null);
		wantsKey = null;
		answer = () => ({ status: 400, json: { error: { message: 'model "llama3" not found' } } });
		const missing = await quickReply({
			provider: 'custom-openai',
			model: 'llama3',
			system: 'Be brief.',
			input: 'Hi',
			maxTokens: 10,
			timeoutMs: 5000
		}).catch((err: unknown) => err);
		expect(describeApiError(missing)).toBe('Custom OpenAI error 400: model "llama3" not found');
	});
});

describe('a chat on a Custom OpenAI model', () => {
	it("runs the agent loop as OpenAI's does, leaving out what only OpenAI has", async () => {
		const { user, chat } = customChat();
		const replies = [streamed([thought, listCall]), streamed([said('One file: a.txt.')])];
		answer = (req) => {
			if (req.path !== RESPONSES) return { status: 404, json: {} };
			return req.json?.stream ? replies.shift()! : titled('Listing files');
		};
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Files?');
		await ended;

		expect(turns()).toHaveLength(2);
		const [first, second] = turns().map((r) => r.json!);
		// No encrypted reasoning or prompt cache key: those are OpenAI's.
		expect(first).toEqual({
			model: 'qwen3:8b',
			instructions: chat.systemPrompt,
			input: [{ role: 'user', content: [{ type: 'input_text', text: 'Anna: Files?' }] }],
			tools: [
				{
					type: 'function',
					name: 'run_command',
					description: RUN_COMMAND_TOOL.description,
					parameters: RUN_COMMAND_TOOL.input_schema,
					strict: false
				}
			],
			store: false,
			stream: true,
			reasoning: { effort: 'medium', summary: 'auto' }
		});
		// Its reasoning has nothing encrypted to send back, so the call goes back without it.
		expect(second.input).toEqual([
			...(first.input as unknown[]),
			listCall,
			{ type: 'function_call_output', call_id: 'call_1', output: 'a.txt\n[exit code 0]' }
		]);
		expect(turns()[0].key).toBe('none');

		const saved = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(JSON.parse(saved[0].content)).toEqual([thought, listCall]);
		expect(toDisplay(saved[0])).toMatchObject({
			stopReason: 'tool_use',
			usage: { input: 900, cacheRead: 0, cacheWrite: 0, output: 30 },
			blocks: [
				{ type: 'thinking', text: 'Anna wants the files.' },
				{ type: 'tool', id: 'call_1', command: 'ls' }
			]
		});
		expect(toDisplay(saved[1])).toMatchObject({
			stopReason: 'end_turn',
			blocks: [{ type: 'text', text: 'One file: a.txt.' }]
		});

		// Named by the same model, without the thoughts it left in the title.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Listing files'));
		const naming = seen.find((r) => r.path === RESPONSES && !r.json?.stream)!.json;
		expect(naming).toEqual({
			model: 'qwen3:8b',
			instructions: expect.any(String),
			input: '<message>\nFiles?\n</message>',
			max_output_tokens: 2048,
			store: false,
			reasoning: { effort: 'low' }
		});
	});

	it("sends OpenAI's levels above high as high, and no reasoning to a model that refuses it", async () => {
		saveCustomOpenai(baseUrl, null);
		answer = (req) =>
			req.json?.reasoning && req.json.model === 'llama3.2'
				? { status: 400, json: { error: { message: '"llama3.2" does not support thinking' } } }
				: streamed([said('Hello.')]);

		await turn('qwen3:8b', 'max');
		expect(turns().at(-1)!.json!.reasoning).toEqual({ effort: 'high', summary: 'auto' });

		const reply = await turn('llama3.2');
		expect(reply.texts).toEqual(['Hello.']);
		// Asked again without, and from then on without.
		await turn('llama3.2');
		expect(turns().map((r) => [r.json!.model, !!r.json!.reasoning])).toEqual([
			['qwen3:8b', true],
			['llama3.2', true],
			['llama3.2', false],
			['llama3.2', false]
		]);
		// Other errors aren't taken for a refusal.
		answer = () => ({ status: 400, json: { error: { message: 'model "gemma" not found' } } });
		const failed = await turn('gemma').catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('Custom OpenAI error 400: model "gemma" not found');
	});

	it('gives the model the paths of pictures and PDFs', async () => {
		const { user, profile, chat } = customChat();
		answer = (req) => (req.json?.stream ? streamed([said('A dot.')]) : titled('A dot'));
		const png = Buffer.from(
			'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
			'base64'
		);
		const picture = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'dot.png',
			body: Readable.from([png])
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What is it?', [picture.id]);
		await ended;

		const input = turns()[0].json!.input as { content: unknown }[];
		expect(input[0].content).toEqual([
			{
				type: 'input_text',
				text: expect.stringMatching(
					/^\[Anna attached dot\.png, saved at .+dot\.png\. It isn't shown here: btw gives models on a Custom OpenAI server only a picture's path\]$/
				)
			},
			{ type: 'input_text', text: 'Anna: What is it?' }
		]);
	});

	it('turns pictures a chat already holds into notes', async () => {
		const messages = [
			{
				role: 'user' as const,
				blocks: [
					{
						type: 'image' as const,
						source: { type: 'inline' as const, mime: 'image/png', data: 'AAAA' }
					}
				]
			}
		];
		expect(await readableMessages('custom-openai', 'qwen3:8b', messages)).toEqual([
			{
				role: 'user',
				blocks: [
					{
						type: 'text',
						text: "[Picture not shown: qwen3:8b can't see pictures. The line before this says where its file is.]"
					}
				]
			}
		]);
	});

	it('says in words when the stream ends early', async () => {
		saveCustomOpenai(baseUrl, null);
		answer = () => ({
			events: [
				{ type: 'response.created', response: { status: 'in_progress' } },
				{ type: 'response.output_text.delta', output_index: 0, delta: 'Hal' }
			]
		});
		const failed = await turn('qwen3:8b').catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('Custom OpenAI: The reply ended before it was complete.');
	});
});
