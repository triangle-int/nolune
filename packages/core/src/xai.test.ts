import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { checkApiKey } from './api-keys.ts';
import { createUpload } from './attachments.ts';
import { initConfig } from './config.ts';
import { committedRows, createConversation, toDisplay } from './conversations.ts';
import {
	describeApiError,
	fetchContextWindow,
	listModels,
	modelInputs,
	pictureTypes,
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

// --- a stand-in for xAI's API ---

interface Seen {
	method: string;
	path: string;
	bearer: string | undefined;
	json: Record<string, unknown> | null;
}
type Answer = { status?: number; json?: unknown; events?: object[] };

const KEY = 'xai-good-000000000000000000000001';

/** `GET /language-models`: what each model takes and writes. */
const LANGUAGE_MODELS = [
	{
		id: 'grok-4.7',
		created: 1_790_000_000,
		aliases: ['grok-latest'],
		input_modalities: ['text', 'image'],
		output_modalities: ['text'],
		capabilities: { reasoning_effort: ['low', 'medium', 'high', 'xhigh'] }
	},
	{
		id: 'grok-4.3',
		created: 1_780_000_000,
		aliases: [],
		input_modalities: ['text', 'image'],
		output_modalities: ['text'],
		capabilities: { reasoning_effort: ['none', 'low', 'medium', 'high', 'xhigh'] }
	},
	{
		id: 'grok-build-0.1',
		created: 1_785_000_000,
		aliases: ['grok-code-fast-1'],
		input_modalities: ['text'],
		output_modalities: ['text']
	},
	{
		id: 'grok-4.20-multi-agent-0309',
		created: 1_775_000_000,
		input_modalities: ['text', 'image'],
		output_modalities: ['text']
	},
	{ id: 'grok-voice', created: 1_789_000_000, output_modalities: ['audio'] }
];

/** `GET /models`: windows and efforts, not what a model takes. */
const MODELS = [
	{ id: 'grok-4.7', context_length: 500_000, created: 1_790_000_000 },
	{ id: 'grok-4.3', context_length: 1_000_000, created: 1_780_000_000 },
	{ id: 'grok-build-0.1', context_length: 256_000, created: 1_785_000_000 },
	{ id: 'grok-4.20-multi-agent-0309', context_length: 1_000_000 }
];

let server: Server;
let root = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;

beforeAll(async () => {
	server = createServer(async (req, res) => {
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk as Buffer);
		const body = Buffer.concat(chunks).toString('utf8');
		const request: Seen = {
			method: req.method ?? 'GET',
			path: req.url ?? '',
			bearer: req.headers.authorization?.replace(/^Bearer /, ''),
			json: body ? (JSON.parse(body) as Seen['json']) : null
		};
		seen.push(request);
		// As xAI answers: a key it doesn't know is a 400, and errors are `{ code, error }`.
		const reply: Answer =
			request.bearer !== KEY
				? {
						status: 400,
						json: {
							code: 'invalid-argument',
							error:
								'Incorrect API key provided. You can obtain an API key from https://console.x.ai.'
						}
					}
				: request.path === '/v1/language-models'
					? { json: { models: LANGUAGE_MODELS } }
					: request.path === '/v1/models'
						? { json: { object: 'list', data: MODELS } }
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
	vi.stubEnv('XAI_BASE_URL', `${root}/v1`);
	vi.stubEnv('XAI_API_KEY', KEY);
	// OpenAI's key, which must never reach xAI.
	vi.stubEnv('OPENAI_API_KEY', 'sk-openai-real-000000000001');
	seen.length = 0;
	answer = () => ({ status: 404, json: { code: 'not-found', error: 'Not found' } });
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

const RESPONSES = '/v1/responses';
const usage = (input: number, output: number, cached = 0) => ({
	input_tokens: input,
	input_tokens_details: { cached_tokens: cached },
	output_tokens: output
});

/** Item by item, a reasoning item's summary as it grows, as xAI streams them. */
function streamed(output: Record<string, unknown>[], used = usage(900, 30)): Answer {
	const events: object[] = [{ type: 'response.created', response: { status: 'in_progress' } }];
	output.forEach((item, index) => {
		events.push({ type: 'response.output_item.added', output_index: index, item });
		if (item.type === 'reasoning') {
			for (const part of item.summary as { text: string }[]) {
				events.push({
					type: 'response.reasoning_summary_text.delta',
					output_index: index,
					summary_index: 0,
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
	summary: [{ type: 'summary_text', text: 'Anna wants the files.' }],
	encrypted_content: 'enc-grok-1'
};
const listArgs = { summary: 'Listing the files', icon: 'folder-open', command: 'ls' };
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
const titled = (title: string): Answer => ({
	json: { status: 'completed', output: [said(title)], usage: usage(60, 4) }
});

/** One call, as the runner makes it. */
const turn = (model: string, effort: Effort = 'medium') =>
	streamTurn({
		provider: 'xai',
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

const turns = () => seen.filter((r) => r.path === RESPONSES);

describe('the xAI key', () => {
	it("is checked with xAI's key endpoint, and one it doesn't know is refused", async () => {
		answer = (req) =>
			req.path === '/v1/api-key'
				? { json: { redacted_api_key: 'xai-…0001', team_blocked: false, api_key_blocked: false } }
				: { status: 404 };
		await expect(checkApiKey('xai', KEY)).resolves.toBe(null);
		expect(seen.at(-1)).toMatchObject({ path: '/v1/api-key', bearer: KEY });

		await expect(checkApiKey('xai', 'xai-wrong')).rejects.toMatchObject({
			reason: 'rejected',
			message: expect.stringContaining("xAI didn't accept this key")
		});
	});

	it('is taken with a warning when its team is blocked', async () => {
		answer = () => ({ json: { team_blocked: true, api_key_blocked: false } });
		await expect(checkApiKey('xai', KEY)).resolves.toContain('its team is blocked');
	});
});

describe("xAI's models", () => {
	it('are the ones that chat and take tools, the newest first, with their windows', async () => {
		expect(await listModels('xai')).toEqual([
			{ id: 'grok-4.7', name: null, description: null, contextWindow: 500_000 },
			{ id: 'grok-build-0.1', name: null, description: null, contextWindow: 256_000 },
			{ id: 'grok-4.3', name: null, description: null, contextWindow: 1_000_000 }
		]);
	});

	it("are checked for a preset, by id or alias, and say what's wrong", async () => {
		expect(await fetchContextWindow('xai', 'grok-4.3')).toBe(1_000_000);
		expect(await fetchContextWindow('xai', 'grok-code-fast-1')).toBe(256_000);

		const missing = await fetchContextWindow('xai', 'grok-9').catch((err: unknown) => err);
		expect(describeApiError(missing)).toBe(
			'Model not found: xAI has no model "grok-9" to chat with for this key. It has grok-4.7, grok-4.3, grok-build-0.1.'
		);
		const agents = await fetchContextWindow('xai', 'grok-4.20-multi-agent-0309').catch(
			(err: unknown) => err
		);
		expect(describeApiError(agents)).toBe(
			"grok-4.20-multi-agent-0309 takes no tools of nolune's, so it can't run commands."
		);
	});

	it('see pictures when xAI says so, as JPEG or PNG, and get PDFs as their paths', async () => {
		expect(await modelInputs('xai', 'grok-4.7')).toEqual({ pictures: true, pdfs: false });
		expect(await modelInputs('xai', 'grok-build-0.1')).toEqual({ pictures: false, pdfs: false });
		expect(pictureTypes('xai')).toEqual(['image/jpeg', 'image/png']);
		expect(pictureTypes('openai')).toBeUndefined();

		const pdf = { type: 'inline' as const, mime: 'application/pdf', data: 'JVBERi0=' };
		const messages = [
			{ role: 'user' as const, blocks: [{ type: 'pdf' as const, name: 'a.pdf', source: pdf }] }
		];
		expect(await readableMessages('xai', 'grok-4.7', messages)).toEqual([
			{
				role: 'user',
				blocks: [
					{
						type: 'text',
						text: "[PDF not shown: grok-4.7 doesn't read PDFs itself. The line before this says where its file is.]"
					}
				]
			}
		]);
	});

	it('get the effort they take: the nearest below, none for a model without any', async () => {
		answer = () => streamed([said('Hello.')]);
		await turn('grok-4.7', 'max');
		await turn('grok-4.3', 'medium');
		await turn('grok-build-0.1', 'high');
		// Not listed: as it is, at most high, and a refusal is learned.
		await turn('grok-5-preview', 'max');
		expect(turns().map((r) => [r.json!.model, r.json!.reasoning, r.json!.include])).toEqual([
			['grok-4.7', { effort: 'xhigh' }, ['reasoning.encrypted_content']],
			['grok-4.3', { effort: 'medium' }, ['reasoning.encrypted_content']],
			// It reasons all the same, and wants its reasoning back.
			['grok-build-0.1', undefined, ['reasoning.encrypted_content']],
			['grok-5-preview', { effort: 'high' }, ['reasoning.encrypted_content']]
		]);
	});

	it('get low effort for chores, or room to reason when they take no effort', async () => {
		answer = () => titled('Hello');
		const chore = (model: string) =>
			quickReply({
				provider: 'xai',
				model,
				system: 'Be brief.',
				input: 'Hi',
				maxTokens: 2048,
				timeoutMs: 5000
			});
		expect((await chore('grok-4.7')).text).toBe('Hello');
		await chore('grok-build-0.1');
		expect(
			turns().map((r) => [r.json!.model, r.json!.reasoning, r.json!.max_output_tokens])
		).toEqual([
			['grok-4.7', { effort: 'low' }, 2048],
			['grok-build-0.1', undefined, 18_048]
		]);
	});

	it('are asked again without an effort they refuse', async () => {
		answer = (req) =>
			req.json?.reasoning
				? {
						status: 400,
						json: {
							code: 'invalid-argument',
							error: 'Model grok-5-preview does not support parameter reasoningEffort.'
						}
					}
				: streamed([said('Hello.')]);
		expect((await turn('grok-5-preview')).texts).toEqual(['Hello.']);
		await turn('grok-5-preview');
		expect(turns().map((r) => !!r.json!.reasoning)).toEqual([true, false, false]);
	});
});

describe('a chat on xAI', () => {
	function chatOn(model = 'grok-4.7') {
		const { user, profile } = makeFamily();
		const preset = makePreset('Grok', model, 'xai');
		return {
			user,
			profile,
			chat: createConversation({ profile, presetId: preset.id, userId: user.id })
		};
	}

	it("runs the agent loop on xAI's Responses API, its reasoning going back to it", async () => {
		const { user, chat } = chatOn();
		const replies = [
			streamed([thought, listCall], usage(900, 30)),
			streamed([said('One file: a.txt.')], usage(950, 12, 800))
		];
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

		const [first, second] = turns().filter((r) => r.json?.stream);
		expect(first.bearer).toBe(KEY);
		expect(first.json).toEqual({
			model: 'grok-4.7',
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
			reasoning: { effort: 'medium' },
			include: ['reasoning.encrypted_content'],
			// Also keeps the chat on the server that has its cache.
			prompt_cache_key: chat.id
		});
		// The reasoning goes back as it came, encrypted, with the call and its result.
		expect(second.json!.input).toEqual([
			...(first.json!.input as unknown[]),
			thought,
			listCall,
			{ type: 'function_call_output', call_id: 'call_1', output: 'a.txt\n[exit code 0]' }
		]);

		const saved = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(toDisplay(saved[0])).toMatchObject({
			stopReason: 'tool_use',
			blocks: [
				{ type: 'thinking', text: 'Anna wants the files.' },
				{ type: 'tool', id: 'call_1', command: 'ls' }
			]
		});
		expect(toDisplay(saved[1])).toMatchObject({
			stopReason: 'end_turn',
			usage: { input: 150, cacheRead: 800, cacheWrite: 0, output: 12 },
			blocks: [{ type: 'text', text: 'One file: a.txt.' }]
		});
	});

	it('sends pictures inline to a model that sees them', async () => {
		const { user, profile, chat } = chatOn();
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

		const first = turns().find((r) => r.json?.stream)!;
		const input = first.json!.input as { content: unknown[] }[];
		expect(input[0].content).toContainEqual({
			type: 'input_image',
			image_url: `data:image/png;base64,${png.toString('base64')}`,
			detail: 'auto'
		});
	});

	it("says what went wrong in xAI's words", async () => {
		vi.stubEnv('XAI_API_KEY', 'xai-wrong');
		const wrong = await turn('grok-4.7').catch((err: unknown) => err);
		expect(describeApiError(wrong)).toBe(
			"xAI didn't accept the API key. An admin can add one under Models & keys in nolune, or with `nolune key set xai`."
		);

		vi.stubEnv('XAI_API_KEY', KEY);
		answer = () => ({
			status: 403,
			json: { code: 'personal-team-blocked:spending-limit', error: 'Your team is out of credits.' }
		});
		const blocked = await turn('grok-4.7').catch((err: unknown) => err);
		expect(describeApiError(blocked)).toBe(
			"xAI turned the request down: Your team is out of credits. See the team's credits and limits at https://console.x.ai."
		);

		vi.stubEnv('XAI_API_KEY', '');
		const none = await turn('grok-4.7').catch((err: unknown) => err);
		expect(describeApiError(none)).toBe(
			'No xAI API key. An admin can add one under Models & keys in nolune, or with `nolune key set xai`.'
		);
	});
});
