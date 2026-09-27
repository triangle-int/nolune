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
	streamTurn
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
type Answer = { status?: number; json?: unknown; chunks?: object[] };

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
		if (reply.chunks) {
			res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream' });
			res.end(
				reply.chunks.map((c) => `data: ${JSON.stringify(c)}\n\n`).join('') + 'data: [DONE]\n\n'
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

function chunk(delta: object, finish: string | null = null) {
	return {
		id: 'chatcmpl-1',
		object: 'chat.completion.chunk',
		choices: [{ index: 0, delta, finish_reason: finish }]
	};
}

const usage = (prompt: number, completion: number) => ({
	id: 'chatcmpl-1',
	object: 'chat.completion.chunk',
	choices: [],
	usage: { prompt_tokens: prompt, completion_tokens: completion, total_tokens: prompt + completion }
});

const listArgs = JSON.stringify({
	summary: 'Listing the files',
	icon: 'folder-open',
	command: 'ls'
});

/** As vLLM and LM Studio stream a model's thinking: `reasoning_content`, then the call. */
const listing: Answer = {
	chunks: [
		chunk({ role: 'assistant', reasoning_content: 'Anna wants ' }),
		chunk({ reasoning_content: 'the files.' }),
		chunk({
			tool_calls: [{ index: 0, id: 'call_1', type: 'function', function: { name: 'run_command' } }]
		}),
		chunk({ tool_calls: [{ index: 0, function: { arguments: listArgs } }] }),
		chunk({}, 'tool_calls'),
		usage(900, 30)
	]
};

const saying = (text: string): Answer => ({
	chunks: [chunk({ role: 'assistant', content: text }), chunk({}, 'stop'), usage(1000, 8)]
});

/** A thinking model's title, its thoughts left in the text as servers without a parser do. */
const titled = (title: string): Answer => ({
	json: {
		id: 'chatcmpl-2',
		object: 'chat.completion',
		choices: [
			{
				index: 0,
				message: { role: 'assistant', content: `<think>\nA short title.\n</think>\n\n${title}` },
				finish_reason: 'stop'
			}
		],
		usage: { prompt_tokens: 60, completion_tokens: 4 }
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

const COMPLETIONS = '/v1/chat/completions';
const turns = () => seen.filter((r) => r.path === COMPLETIONS && r.json?.stream);

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
	it('runs the agent loop over Chat Completions, sending only what such servers take', async () => {
		const { user, chat } = customChat();
		const replies = [listing, saying('One file: a.txt.')];
		answer = (req) => {
			if (req.path !== COMPLETIONS) return { status: 404, json: {} };
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
		expect(first).toEqual({
			model: 'qwen3:8b',
			messages: [
				{ role: 'system', content: chat.systemPrompt },
				{ role: 'user', content: [{ type: 'text', text: 'Anna: Files?' }] }
			],
			tools: [
				{
					type: 'function',
					function: {
						name: 'run_command',
						description: RUN_COMMAND_TOOL.description,
						parameters: RUN_COMMAND_TOOL.input_schema
					}
				}
			],
			stream: true,
			stream_options: { include_usage: true }
		});
		// Its reply goes back without its reasoning, which such servers don't take.
		expect(second.messages).toEqual([
			...(first.messages as unknown[]),
			{
				role: 'assistant',
				content: null,
				tool_calls: [
					{ id: 'call_1', type: 'function', function: { name: 'run_command', arguments: listArgs } }
				]
			},
			{ role: 'tool', tool_call_id: 'call_1', content: 'a.txt\n[exit code 0]' }
		]);
		expect(turns()[0].key).toBe('none');

		const saved = committedRows(chat.id).filter((row) => row.kind === 'assistant');
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
		const naming = seen.find((r) => r.path === COMPLETIONS && !r.json?.stream)!.json;
		expect(naming).toEqual({
			model: 'qwen3:8b',
			messages: [
				{ role: 'system', content: expect.any(String) },
				{ role: 'user', content: '<message>\nFiles?\n</message>' }
			],
			max_tokens: 2048
		});
	});

	it('gives the model the paths of pictures and PDFs', async () => {
		const { user, profile, chat } = customChat();
		answer = (req) => (req.json?.stream ? saying('A dot.') : titled('A dot'));
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

		const content = (turns()[0].json!.messages as { content: unknown }[])[1].content;
		expect(content).toEqual([
			{
				type: 'text',
				text: expect.stringMatching(
					/^\[Anna attached dot\.png, saved at .+dot\.png\. It isn't shown here: btw gives models on a Custom OpenAI server only a picture's path\]$/
				)
			},
			{ type: 'text', text: 'Anna: What is it?' }
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
		answer = () => ({ chunks: [chunk({ role: 'assistant', content: 'Hal' })] });
		const failed = await streamTurn({
			provider: 'custom-openai',
			model: 'qwen3:8b',
			effort: 'medium',
			system: 'Be brief.',
			tools: [],
			cacheTtl: '1h',
			cacheKey: 'conv-1',
			messages: [{ role: 'user', blocks: [{ type: 'text', text: 'Hi' }] }],
			signal: new AbortController().signal,
			onEvent: () => {}
		}).catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('Custom OpenAI: the reply ended before it was complete.');
	});
});
