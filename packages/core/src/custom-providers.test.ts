import { createServer, type Server as HttpServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import { initConfig, readConfig } from './config.ts';
import { committedRows, createConversation, getConversation, toDisplay } from './conversations.ts';
import {
	CustomProviderError,
	anthropicUrl,
	checkCustomProvider,
	findCustomProvider,
	listCustomProviders,
	openaiUrl,
	removeCustomProvider,
	saveCustomProvider,
	splitModel,
	suggestProviderName,
	type CustomApi
} from './custom-providers.ts';
import {
	describeApiError,
	fetchContextWindow,
	listModels,
	quickReply,
	readableMessages,
	streamTurn,
	type Effort,
	type Provider
} from './models.ts';
import { addPreset } from './presets.ts';
import { RUN_COMMAND_TOOL, runCommand } from './run-command.ts';
import { onLoopEnd, sendMessage } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

// --- a stand-in for a model server that speaks both APIs, like Ollama or LM Studio ---

interface Seen {
	method: string;
	path: string;
	/** The bearer token, and Anthropic's header. */
	bearer: string | undefined;
	apiKey: string | undefined;
	json: Record<string, unknown> | null;
}
type Answer = { status?: number; json?: unknown; events?: object[] };

let server: HttpServer;
/** The server's own address, as it's added: OpenAI's API is under /v1. */
let root = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;
/** The key it wants, if any, which it takes either way. */
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
			bearer: req.headers.authorization?.replace(/^Bearer /, ''),
			apiKey: req.headers['x-api-key'] as string | undefined,
			json: body ? (JSON.parse(body) as Seen['json']) : null
		};
		seen.push(request);
		const given = request.bearer ?? request.apiKey;
		const reply: Answer =
			wantsKey && given !== wantsKey
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
	root = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	initConfig();
	// The real providers' keys, which must never reach a custom provider.
	vi.stubEnv('OPENAI_API_KEY', 'sk-openai-real-000000000001');
	vi.stubEnv('ANTHROPIC_API_KEY', 'sk-ant-real-000000000001');
	vi.stubEnv('ANTHROPIC_AUTH_TOKEN', 'sk-ant-token-000000000001');
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

function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

/** A custom provider at the stand-in (or `url`), its id from its name. */
const add = (name: string, api: CustomApi, url = root, key: string | null = null) =>
	saveCustomProvider({ name, api, url, key });

function chatOn(provider: Provider, model = 'local/qwen3:8b') {
	add('Local', provider === 'custom-anthropic' ? 'anthropic' : 'openai');
	const { user, profile } = makeFamily();
	const preset = makePreset('On the server', model, provider);
	return {
		user,
		profile,
		chat: createConversation({ profile, presetId: preset.id, userId: user.id })
	};
}

/** One call on a custom provider's model, as the runner makes it. */
const turn = (provider: Provider, model: string, effort: Effort = 'medium') =>
	streamTurn({
		provider,
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

const quick = (provider: Provider, model: string) =>
	quickReply({
		provider,
		model,
		system: 'Be brief.',
		input: 'Hi',
		maxTokens: 10,
		timeoutMs: 5000
	});

const listArgs = { summary: 'Listing the files', icon: 'folder-open', command: 'ls' };

// --- OpenAI's Responses API, as vLLM and LM Studio stream it ---

const RESPONSES = '/v1/responses';
const usage = (input: number, output: number) => ({ input_tokens: input, output_tokens: output });

/** Item by item, a reasoning item's full text (`reasoning_text`) where OpenAI gives a summary. */
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
const listCall = {
	id: 'fc_1',
	type: 'function_call',
	status: 'completed',
	call_id: 'call_1',
	name: 'run_command',
	arguments: JSON.stringify(listArgs)
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

// --- Anthropic's Messages API, as llama.cpp and Ollama stream it ---

const MESSAGES = '/v1/messages';

/** A streamed message, block by block. */
function message(
	blocks: Record<string, unknown>[],
	stop: string,
	used = { input_tokens: 700, output_tokens: 25 }
): Answer {
	const events: object[] = [
		{
			type: 'message_start',
			message: {
				id: 'msg_1',
				type: 'message',
				role: 'assistant',
				model: 'qwen3:8b',
				content: [],
				stop_reason: null,
				stop_sequence: null,
				usage: { input_tokens: used.input_tokens, output_tokens: 1 }
			}
		}
	];
	blocks.forEach((block, index) => {
		if (block.type === 'text') {
			events.push({
				type: 'content_block_start',
				index,
				content_block: { type: 'text', text: '' }
			});
			events.push({
				type: 'content_block_delta',
				index,
				delta: { type: 'text_delta', text: block.text }
			});
		} else if (block.type === 'thinking') {
			events.push({
				type: 'content_block_start',
				index,
				content_block: { type: 'thinking', thinking: '', signature: '' }
			});
			events.push({
				type: 'content_block_delta',
				index,
				delta: { type: 'thinking_delta', thinking: block.thinking }
			});
		} else {
			events.push({
				type: 'content_block_start',
				index,
				content_block: { type: 'tool_use', id: block.id, name: block.name, input: {} }
			});
			events.push({
				type: 'content_block_delta',
				index,
				delta: { type: 'input_json_delta', partial_json: JSON.stringify(block.input) }
			});
		}
		events.push({ type: 'content_block_stop', index });
	});
	events.push({
		type: 'message_delta',
		delta: { stop_reason: stop, stop_sequence: null },
		usage: { output_tokens: used.output_tokens }
	});
	events.push({ type: 'message_stop' });
	return { events };
}

const saidAnthropic = (text: string): Answer => ({
	json: {
		id: 'msg_2',
		type: 'message',
		role: 'assistant',
		model: 'qwen3:8b',
		content: [{ type: 'text', text }],
		stop_reason: 'end_turn',
		stop_sequence: null,
		usage: { input_tokens: 60, output_tokens: 4 }
	}
});

// --- tests ---

describe('custom providers', () => {
	it('are kept with an id from their name, their keys never shown, and a key only for its address', () => {
		expect(listCustomProviders()).toEqual([]);
		expect(add('Ollama', 'openai', 'http://localhost:11434/')).toBe('ollama');
		expect(
			add('GPU box', 'anthropic', 'http://gpu-box:8000/v1', 'sk-gpu-key-0000000000000042')
		).toBe('gpu-box');
		expect(listCustomProviders()).toEqual([
			{
				id: 'ollama',
				name: 'Ollama',
				api: 'openai',
				url: 'http://localhost:11434',
				hasKey: false,
				hint: null
			},
			{
				id: 'gpu-box',
				name: 'GPU box',
				api: 'anthropic',
				url: 'http://gpu-box:8000/v1',
				hasKey: true,
				hint: '0042'
			}
		]);
		expect(JSON.stringify(listCustomProviders())).not.toContain('sk-gpu');
		expect(findCustomProvider('gpu BOX')?.key).toBe('sk-gpu-key-0000000000000042');

		// Its name, address and key change; its id and API stay, as its presets' models need them.
		// A form that never showed the key keeps it for the same address, not for another.
		saveCustomProvider({
			id: 'gpu-box',
			name: 'Big GPU',
			api: 'openai',
			url: 'http://gpu-box:8000/v1',
			key: undefined
		});
		expect(findCustomProvider('big gpu')).toEqual({
			id: 'gpu-box',
			name: 'Big GPU',
			api: 'anthropic',
			url: 'http://gpu-box:8000/v1',
			key: 'sk-gpu-key-0000000000000042'
		});
		saveCustomProvider({
			id: 'gpu-box',
			name: 'Big GPU',
			url: 'http://other-box:8000/v1',
			key: undefined
		});
		expect(findCustomProvider('gpu-box')).toEqual({
			id: 'gpu-box',
			name: 'Big GPU',
			api: 'anthropic',
			url: 'http://other-box:8000/v1'
		});
		// Its old name is free again, with another id.
		expect(add('GPU box', 'openai')).toBe('gpu-box-2');
		removeCustomProvider('gpu-box-2');

		// One name each, and not a built-in provider's.
		expect(() => add('ollama', 'anthropic')).toThrow("There's already a provider called Ollama.");
		expect(() => add('OpenAI', 'openai')).toThrow("There's already a provider called OpenAI.");
		expect(() => add('!!', 'openai')).toThrow(CustomProviderError);

		removeCustomProvider('gpu-box');
		expect(readConfig().customProviders).toEqual([
			{ id: 'ollama', name: 'Ollama', api: 'openai', url: 'http://localhost:11434' }
		]);
		removeCustomProvider('ollama');
		expect(readConfig().customProviders).toBeUndefined();
	});

	it('say where each API is, and name models as <id>/<model>', () => {
		expect(openaiUrl('http://localhost:11434')).toBe('http://localhost:11434/v1');
		expect(openaiUrl('http://localhost:11434/v1/')).toBe('http://localhost:11434/v1');
		expect(openaiUrl('https://proxy.example.com/llm/v1')).toBe('https://proxy.example.com/llm/v1');
		expect(anthropicUrl('http://localhost:11434/v1')).toBe('http://localhost:11434');
		expect(anthropicUrl('http://localhost:8080')).toBe('http://localhost:8080');

		expect(splitModel('gpu-box/Qwen/Qwen3-32B')).toEqual({
			provider: 'gpu-box',
			model: 'Qwen/Qwen3-32B'
		});
		expect(splitModel('qwen3:8b')).toEqual({ provider: '', model: 'qwen3:8b' });

		expect(suggestProviderName('http://localhost:11434')).toBe('Local');
		expect(suggestProviderName('http://127.0.0.1:1234/v1')).toBe('Local');
		expect(suggestProviderName('http://gpu-box.lan:8000/v1')).toBe('gpu-box');
	});

	it('are checked by asking them for their models, with the key both ways', async () => {
		expect(await checkCustomProvider(root, null)).toEqual({
			models: ['qwen3:8b', 'Qwen/Qwen3-32B'],
			warning: null
		});
		expect(seen.at(-1)).toMatchObject({ path: '/v1/models', bearer: undefined, apiKey: undefined });

		wantsKey = 'secret';
		await expect(checkCustomProvider(root, null)).rejects.toMatchObject({
			reason: 'key',
			message: 'The server wants a key.'
		});
		await expect(checkCustomProvider(root, 'wrong')).rejects.toMatchObject({ reason: 'key' });
		expect((await checkCustomProvider(`${root}/v1/`, 'secret')).models).toHaveLength(2);
		expect(seen.at(-1)).toMatchObject({ path: '/v1/models', bearer: 'secret', apiKey: 'secret' });

		models = [];
		wantsKey = null;
		expect(await checkCustomProvider(root, null)).toEqual({
			models: [],
			warning: "It doesn't list any models yet."
		});
		await expect(checkCustomProvider('http://127.0.0.1:1', null)).rejects.toMatchObject({
			reason: 'unreachable'
		});
		await expect(checkCustomProvider('localhost:11434', null)).rejects.toBeInstanceOf(
			CustomProviderError
		);
	});

	it("list their models for a preset, with a window where they say one, and check a preset's", async () => {
		add('Local', 'anthropic');
		expect(await listModels('custom-anthropic', 'local')).toEqual([
			{ id: 'local/qwen3:8b', name: 'qwen3:8b', description: null, contextWindow: null },
			{
				id: 'local/Qwen/Qwen3-32B',
				name: 'Qwen/Qwen3-32B',
				description: null,
				contextWindow: 40_960
			}
		]);
		expect(await fetchContextWindow('custom-anthropic', 'local/Qwen/Qwen3-32B')).toBe(40_960);
		expect(await fetchContextWindow('custom-anthropic', 'local/qwen3:8b')).toBeNull();
		await expect(fetchContextWindow('custom-anthropic', 'local/llama3')).rejects.toThrow(
			'Local has no model "llama3". It serves qwen3:8b, Qwen/Qwen3-32B.'
		);
		await expect(fetchContextWindow('custom-anthropic', 'gpu/llama3')).rejects.toThrow(
			'No custom provider "gpu". Custom providers: Local.'
		);
		// Its API is its own.
		await expect(fetchContextWindow('custom-openai', 'local/qwen3:8b')).rejects.toThrow(
			"Local speaks Anthropic's API, not OpenAI's."
		);
		// One that lists none can't be asked: whatever it serves shows at the first reply.
		models = [];
		expect(await fetchContextWindow('custom-anthropic', 'local/llama3')).toBeNull();

		// A preset on it is named by its name.
		models = [{ id: 'qwen3:8b' }];
		const preset = await addPreset({ provider: 'custom-anthropic', model: 'local/qwen3:8b' });
		expect(preset.name).toBe('qwen3:8b (Local)');
	});

	it('say what went wrong in words: no such provider, the key, or one that is down', async () => {
		const missing = await quick('custom-openai', 'local/qwen3:8b').catch((err: unknown) => err);
		expect(describeApiError(missing)).toBe(
			'No custom provider "local". An admin can add one under Models & keys in btw, or with `btw provider add`.'
		);
		const unnamed = await quick('custom-openai', 'qwen3:8b').catch((err: unknown) => err);
		expect(describeApiError(unnamed)).toContain("doesn't say which custom provider it's on");

		add('Local', 'openai');
		add('oMLX', 'anthropic');
		wantsKey = 'secret';
		for (const [provider, model] of [
			['custom-openai', 'local/qwen3:8b'],
			['custom-anthropic', 'omlx/qwen3:8b']
		] as const) {
			const refused = await quick(provider, model).catch((err: unknown) => err);
			const name = provider === 'custom-openai' ? 'Local' : 'oMLX';
			expect(describeApiError(refused)).toBe(
				`${name} at ${root} didn't accept the key. An admin can change it under Models & keys in btw.`
			);
		}

		add('Down', 'anthropic', 'http://127.0.0.1:1');
		const down = await turn('custom-anthropic', 'down/qwen3:8b').catch((err: unknown) => err);
		expect(describeApiError(down)).toMatch(
			/^Couldn't reach Down at http:\/\/127\.0\.0\.1:1 \(.+\)\. Is it running\?$/
		);

		wantsKey = null;
		answer = (req) =>
			req.path === MESSAGES
				? {
						status: 404,
						json: { type: 'error', error: { type: 'not_found_error', message: 'no model "x"' } }
					}
				: { status: 400, json: { error: { message: 'model "llama3" not found' } } };
		const notFound = await quick('custom-openai', 'local/llama3').catch((err: unknown) => err);
		expect(describeApiError(notFound)).toBe('Local error 400: model "llama3" not found');
		const other = await quick('custom-anthropic', 'omlx/x').catch((err: unknown) => err);
		expect(describeApiError(other)).toBe('oMLX error 404: no model "x"');
	});
});

describe("a chat on a custom provider of OpenAI's API", () => {
	it("runs the agent loop as OpenAI's does, leaving out what only OpenAI has", async () => {
		const { user, chat } = chatOn('custom-openai');
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

		const turns = seen.filter((r) => r.path === RESPONSES && r.json?.stream);
		expect(turns).toHaveLength(2);
		const [first, second] = turns.map((r) => r.json!);
		// The model as the server knows it; no encrypted reasoning or prompt cache key.
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
		// No key: OPENAI_API_KEY stays OpenAI's.
		expect(turns[0].bearer).toBe('none');

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
		add('Local', 'openai');
		answer = (req) =>
			req.json?.reasoning && req.json.model === 'llama3.2'
				? { status: 400, json: { error: { message: '"llama3.2" does not support thinking' } } }
				: streamed([said('Hello.')]);

		await turn('custom-openai', 'local/qwen3:8b', 'max');
		expect(seen.at(-1)!.json!.reasoning).toEqual({ effort: 'high', summary: 'auto' });

		const reply = await turn('custom-openai', 'local/llama3.2');
		expect(reply.texts).toEqual(['Hello.']);
		// Asked again without, and from then on without.
		await turn('custom-openai', 'local/llama3.2');
		expect(seen.map((r) => [r.json!.model, !!r.json!.reasoning])).toEqual([
			['qwen3:8b', true],
			['llama3.2', true],
			['llama3.2', false],
			['llama3.2', false]
		]);
		// Other errors aren't taken for a refusal.
		answer = () => ({ status: 400, json: { error: { message: 'model "gemma" not found' } } });
		const failed = await turn('custom-openai', 'local/gemma').catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('Local error 400: model "gemma" not found');
	});

	it('gives the model the paths of pictures and PDFs', async () => {
		const { user, profile, chat } = chatOn('custom-openai');
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

		const first = seen.find((r) => r.path === RESPONSES && r.json?.stream)!;
		const input = first.json!.input as { content: unknown }[];
		expect(input[0].content).toEqual([
			{
				type: 'input_text',
				text: expect.stringMatching(
					/^\[Anna attached dot\.png, saved at .+dot\.png\. It isn't shown here: btw gives models of custom providers only a picture's path\]$/
				)
			},
			{ type: 'input_text', text: 'Anna: What is it?' }
		]);
	});

	it('turns pictures a chat already holds into notes, and says when a stream ends early', async () => {
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
		expect(await readableMessages('custom-anthropic', 'local/qwen3:8b', messages)).toEqual([
			{
				role: 'user',
				blocks: [
					{
						type: 'text',
						text: "[Picture not shown: local/qwen3:8b can't see pictures. The line before this says where its file is.]"
					}
				]
			}
		]);

		add('Local', 'openai');
		answer = () => ({
			events: [
				{ type: 'response.created', response: { status: 'in_progress' } },
				{ type: 'response.output_text.delta', output_index: 0, delta: 'Hal' }
			]
		});
		const failed = await turn('custom-openai', 'local/qwen3:8b').catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('Local: The reply ended before it was complete.');
	});
});

describe("a chat on a custom provider of Anthropic's API", () => {
	it("runs the agent loop as Anthropic's does, leaving out what only Anthropic has", async () => {
		const { user, chat } = chatOn('custom-anthropic');
		const replies = [
			message(
				[
					{ type: 'thinking', thinking: 'Anna wants the files.' },
					{ type: 'tool_use', id: 'toolu_1', name: 'run_command', input: listArgs }
				],
				'tool_use'
			),
			message([{ type: 'text', text: 'One file: a.txt.' }], 'end_turn')
		];
		answer = (req) => {
			if (req.path !== MESSAGES) return { status: 404, json: {} };
			return req.json?.stream ? replies.shift()! : saidAnthropic('<think>hm</think>Listing files');
		};
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Files?');
		await ended;

		const turns = seen.filter((r) => r.path === MESSAGES && r.json?.stream);
		expect(turns).toHaveLength(2);
		const [first, second] = turns.map((r) => r.json!);
		// The model as the server knows it; no cache marks, adaptive thinking or effort.
		expect(first).toEqual({
			model: 'qwen3:8b',
			max_tokens: 32_000,
			system: chat.systemPrompt,
			tools: [RUN_COMMAND_TOOL],
			messages: [{ role: 'user', content: [{ type: 'text', text: 'Anna: Files?' }] }],
			stream: true
		});
		// Its thinking has no signature to be checked by, so the call goes back without it.
		expect(second.messages).toEqual([
			...(first.messages as unknown[]),
			{
				role: 'assistant',
				content: [{ type: 'tool_use', id: 'toolu_1', name: 'run_command', input: listArgs }]
			},
			{
				role: 'user',
				content: [{ type: 'tool_result', tool_use_id: 'toolu_1', content: 'a.txt\n[exit code 0]' }]
			}
		]);
		// No key: ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN stay Anthropic's.
		expect(turns[0]).toMatchObject({ apiKey: 'none', bearer: undefined });

		const saved = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(toDisplay(saved[0])).toMatchObject({
			stopReason: 'tool_use',
			usage: { input: 700, cacheRead: 0, cacheWrite: 0, output: 25 },
			blocks: [
				{ type: 'thinking', text: 'Anna wants the files.' },
				{ type: 'tool', id: 'toolu_1', command: 'ls' }
			]
		});
		expect(toDisplay(saved[1])).toMatchObject({
			stopReason: 'end_turn',
			blocks: [{ type: 'text', text: 'One file: a.txt.' }]
		});

		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Listing files'));
		const naming = seen.find((r) => r.path === MESSAGES && !r.json?.stream)!.json;
		expect(naming).toEqual({
			model: 'qwen3:8b',
			max_tokens: 2048,
			system: expect.any(String),
			messages: [{ role: 'user', content: '<message>\nFiles?\n</message>' }]
		});
	});

	it('sends its key both ways, at the server itself rather than /v1', async () => {
		add('GPU box', 'anthropic', `${root}/v1`, 'sk-gpu-key-0000000000000042');
		wantsKey = 'sk-gpu-key-0000000000000042';
		answer = () => message([{ type: 'text', text: 'Hi.' }], 'end_turn');
		const reply = await turn('custom-anthropic', 'gpu-box/qwen3:8b');
		expect(reply.texts).toEqual(['Hi.']);
		expect(seen.at(-1)).toMatchObject({
			path: MESSAGES,
			apiKey: 'sk-gpu-key-0000000000000042',
			bearer: 'sk-gpu-key-0000000000000042'
		});
	});
});
