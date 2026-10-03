import { existsSync, readFileSync, statSync } from 'node:fs';
import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { committedRows, createConversation } from './conversations.ts';
import { generateImages, imageGenerationStatus } from './image-generation.ts';
import { embeddingSource } from './memory-embeddings.ts';
import { listModels } from './models.ts';
import {
	cancelNolunePlanSignIn,
	nolunePlanAccountUrl,
	nolunePlanSignInState,
	nolunePlanStatus,
	nolunePlanUsage,
	onNolunePlanUsage,
	planRefusal,
	signOutNolunePlan,
	startNolunePlanSignIn
} from './nolune-plan.ts';
import { paths } from './paths.ts';
import { runCommand } from './run-command.ts';
import { getSnapshot, onLoopEnd, sendMessage } from './runner.ts';
import { makeFamily, makePreset, runCommandsUnchecked } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

/*
 * The nolune plan against a stand-in for nolune's API (packages/api): its device codes, the token
 * a linked gateway gets, its usage, and the Chat Completions and embeddings it passes on to
 * OpenRouter. The link page's part (someone approving the code) is played by the tests.
 */

interface Seen {
	method: string;
	path: string;
	headers: IncomingHttpHeaders;
	json: Record<string, unknown> | null;
}
type Answer = {
	status?: number;
	headers?: Record<string, string>;
	json?: unknown;
	events?: object[];
};

let server: Server;
let base = '';
const seen: Seen[] = [];
/** What the link page did with the code: nothing yet, linked it, or turned it down. */
let decision: 'pending' | 'approved' | 'denied' = 'pending';
/** The chat endpoint's answers, in turn. */
let chats: Answer[] = [];
/** Whether the account the gateway is linked to has a plan (`/v1/usage` says so). */
let subscribed = true;
/** The picture endpoint's answers, in turn; a PNG when there are none left. */
let pictures: Answer[] = [];

const TOKEN = 'session-token-1';
const USAGE = {
	window: null,
	week: null,
	month: { spent: 0, limit: 25_000_000, resetsAt: Date.UTC(2026, 9, 31) },
	credits: { plan: 25_000_000, extra: 0, renewsAt: Date.UTC(2026, 9, 31) }
};
const IMAGE_MODELS = [
	{
		id: 'openai/gpt-image-2.5-flare',
		architecture: { input_modalities: ['text', 'image'], output_modalities: ['image'] },
		supported_parameters: {
			aspect_ratio: { type: 'enum', values: ['1:1', '3:2', '2:3', 'auto'] },
			quality: { type: 'enum', values: ['auto', 'low', 'medium', 'high'] },
			n: { type: 'range', min: 1, max: 1 },
			input_references: { type: 'range', min: 0, max: 16 }
		}
	}
];
const MODELS = [
	{
		id: 'anthropic/claude-haiku-4.5',
		name: 'Anthropic: Claude Haiku 4.5',
		created: 1_760_000_000,
		context_length: 200_000,
		supported_parameters: ['tools', 'reasoning'],
		architecture: { input_modalities: ['text', 'image'] }
	},
	// Newer, but the API lists its pick first.
	{
		id: 'someone/newer-model',
		created: 1_770_000_000,
		context_length: 100_000,
		supported_parameters: ['tools']
	}
];

function answer(request: Seen): Answer {
	const signedIn = request.headers.authorization === `Bearer ${TOKEN}`;
	switch (`${request.method} ${request.path}`) {
		case 'POST /api/auth/device/code':
			return {
				json: {
					device_code: 'device-1',
					user_code: 'CW53R2HT',
					verification_uri: `${base}/link`,
					verification_uri_complete: `${base}/link?user_code=CW53R2HT`,
					expires_in: 900,
					interval: 0.01
				}
			};
		case 'POST /api/auth/device/token':
			if (decision === 'approved') {
				return { json: { access_token: TOKEN, token_type: 'Bearer', expires_in: 7_776_000 } };
			}
			return {
				status: 400,
				json: { error: decision === 'denied' ? 'access_denied' : 'authorization_pending' }
			};
		case 'POST /api/auth/sign-out':
			return { json: { success: true } };
	}
	if (!signedIn)
		return { status: 401, json: { error: { code: 'not_signed_in', message: 'Link again.' } } };
	switch (`${request.method} ${request.path}`) {
		case 'GET /v1/usage':
			return { json: { email: 'anna@example.com', usage: subscribed ? USAGE : null } };
		case 'GET /v1/models':
			return { json: { data: MODELS } };
		case 'POST /v1/embeddings':
			return { json: { data: [{ index: 0, embedding: [1, 0] }], usage: { cost: 0 } } };
		case 'GET /v1/images/models':
			return { json: { data: IMAGE_MODELS } };
		case 'POST /v1/images':
			return (
				pictures.shift() ?? {
					json: {
						created: 0,
						data: [
							{ b64_json: Buffer.from('a picture').toString('base64'), media_type: 'image/png' }
						],
						usage: { cost: 0.04 }
					}
				}
			);
		case 'POST /v1/chat/completions':
			if (request.json?.stream !== true) {
				// A chore (the chat's title): one reply, not streamed.
				return {
					json: {
						id: 'gen-title',
						choices: [{ message: { role: 'assistant', content: 'Files' }, finish_reason: 'stop' }],
						usage: { prompt_tokens: 5, completion_tokens: 1 }
					}
				};
			}
			return chats.shift() ?? { status: 500, json: { error: { message: 'no answer scripted' } } };
	}
	return { status: 404, json: { error: { message: 'not here' } } };
}

beforeAll(async () => {
	server = createServer((req, res) => {
		let raw = '';
		req.on('data', (chunk) => (raw += chunk));
		req.on('end', () => {
			const url = new URL(req.url ?? '/', 'http://stand-in');
			const request: Seen = {
				method: req.method ?? 'GET',
				path: url.pathname,
				headers: req.headers,
				json: raw ? (JSON.parse(raw) as Record<string, unknown>) : null
			};
			seen.push(request);
			const { status = 200, headers = {}, json, events } = answer(request);
			if (events) {
				res.writeHead(status, { 'content-type': 'text/event-stream', ...headers });
				for (const event of events) res.write(`data: ${JSON.stringify(event)}\n\n`);
				res.end('data: [DONE]\n\n');
				return;
			}
			res.writeHead(status, { 'content-type': 'application/json', ...headers });
			res.end(JSON.stringify(json ?? {}));
		});
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
	process.env.NOLUNE_PLAN_API_URL = base;
});

afterAll(() => {
	delete process.env.NOLUNE_PLAN_API_URL;
	server.close();
});

beforeEach(() => {
	seen.length = 0;
	decision = 'pending';
	chats = [];
	pictures = [];
	subscribed = true;
	cancelNolunePlanSignIn();
	vi.mocked(runCommand).mockReset();
	vi.stubEnv('OPENAI_API_KEY', '');
	vi.stubEnv('OPENROUTER_API_KEY', '');
});

/** Links nolune, someone approving the code as soon as it's asked for. */
async function link(): Promise<void> {
	decision = 'approved';
	await (
		await startNolunePlanSignIn()
	).done;
}

function chatCalls(): Seen[] {
	return seen.filter((r) => r.path === '/v1/chat/completions' && r.json?.stream === true);
}

function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

function planChat() {
	runCommandsUnchecked();
	const { user, profile } = makeFamily();
	const preset = makePreset('Haiku', 'anthropic/claude-haiku-4.5', 'nolune-plan');
	const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
	return { user, chat };
}

describe('linking nolune to the nolune plan', () => {
	it('shows a code to approve, waits for it, and keeps the token for this user only', async () => {
		const signIn = await startNolunePlanSignIn();
		expect(signIn.code).toBe('CW53-R2HT');
		expect(signIn.url).toBe(`${base}/link?user_code=CW53R2HT`);
		expect(nolunePlanSignInState().pending).toMatchObject({ code: 'CW53-R2HT' });
		expect(seen[0].json).toEqual({ client_id: 'nolune' });

		await vi.waitFor(() =>
			expect(seen.filter((r) => r.path === '/api/auth/device/token').length).toBeGreaterThan(1)
		);
		expect(existsSync(paths.nolunePlan)).toBe(false);
		decision = 'approved';
		await signIn.done;

		expect(JSON.parse(readFileSync(paths.nolunePlan, 'utf8'))).toMatchObject({
			api: base,
			token: TOKEN,
			email: 'anna@example.com'
		});
		expect(statSync(paths.nolunePlan).mode & 0o777).toBe(0o600);
		expect(nolunePlanSignInState()).toEqual({ pending: null, signInError: null });
		expect(await nolunePlanStatus({ check: true })).toMatchObject({
			signedIn: 'signed in as anna@example.com',
			usage: USAGE,
			problem: null
		});
	});

	it('says so when the code is turned down on the link page', async () => {
		const signIn = await startNolunePlanSignIn();
		decision = 'denied';
		await expect(signIn.done).rejects.toThrow('Not linked: it was turned down on the link page.');
		expect(nolunePlanSignInState().signInError).toBe(
			'Not linked: it was turned down on the link page.'
		);
		expect(existsSync(paths.nolunePlan)).toBe(false);
	});

	it('forgets a token the API no longer takes, and signs out telling the API', async () => {
		await link();
		await signOutNolunePlan();
		expect(seen.at(-1)).toMatchObject({
			method: 'POST',
			path: '/api/auth/sign-out',
			headers: { authorization: `Bearer ${TOKEN}` }
		});
		expect(existsSync(paths.nolunePlan)).toBe(false);
		expect((await nolunePlanStatus()).problem).toMatch(/^nolune isn't linked to a nolune plan\./);
	});
});

describe('chats on the nolune plan', () => {
	it("runs a turn in nolune's own loop on nolune's API, saying what each request is for", async () => {
		await link();
		const { user, chat } = planChat();
		chats = [
			{
				events: [
					{
						id: 'gen-1',
						choices: [
							{
								delta: {
									tool_calls: [
										{
											index: 0,
											id: 'call_1',
											function: { name: 'run_command', arguments: '{"command":"ls"}' }
										}
									]
								}
							}
						]
					},
					{
						id: 'gen-1',
						choices: [{ delta: {}, finish_reason: 'tool_calls' }],
						usage: { prompt_tokens: 50, completion_tokens: 5 }
					}
				]
			},
			{
				events: [
					{ id: 'gen-2', choices: [{ delta: { content: 'One file: a.txt.' } }] },
					{
						id: 'gen-2',
						choices: [{ delta: {}, finish_reason: 'stop' }],
						usage: { prompt_tokens: 60, completion_tokens: 6 }
					}
				]
			}
		];
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Files?');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		expect(committedRows(chat.id).map((row) => row.kind)).toEqual([
			'human',
			'assistant',
			'tool_results',
			'assistant'
		]);
		const [first, second] = chatCalls();
		expect(first.headers).toMatchObject({
			authorization: `Bearer ${TOKEN}`,
			'x-nolune-use': 'person'
		});
		expect(first.headers['x-nolune-turn']).toBeUndefined();
		expect(second.headers).toMatchObject({ 'x-nolune-use': 'person', 'x-nolune-turn': 'continue' });
		expect(first.json).toMatchObject({ model: 'anthropic/claude-haiku-4.5', session_id: chat.id });
		// The reply went back to the model that wrote it as it came, the plan's own.
		const replies = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(replies.map((row) => row.provider)).toEqual(['nolune-plan', 'nolune-plan']);
	});

	it('asks where the limits stand once a request has ended, and tells who listens', async () => {
		await link();
		const { user, chat } = planChat();
		chats = [
			{
				events: [
					{ id: 'gen-3', choices: [{ delta: { content: 'Hi!' } }] },
					{ id: 'gen-3', choices: [{ delta: {}, finish_reason: 'stop' }] }
				]
			}
		];
		const heard = vi.fn();
		const off = onNolunePlanUsage(heard);
		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Hi');
		await ended;

		await vi.waitFor(() => expect(nolunePlanUsage()?.usage).toEqual(USAGE), { timeout: 5000 });
		expect(heard).toHaveBeenCalled();
		// Once, for the reply and the title that ended together.
		expect(seen.filter((r) => r.path === '/v1/usage').length).toBe(2);
		off();

		await signOutNolunePlan();
		expect(nolunePlanUsage()).toBeNull();
	});

	it('says once that the account has no plan, and where to subscribe, without asking on and on', async () => {
		await link();
		subscribed = false;
		const heard = vi.fn();
		const off = onNolunePlanUsage(heard);
		const status = await nolunePlanStatus({ check: true });
		expect(status).toMatchObject({ noPlan: true, usage: null });
		expect(status.problem).toContain(`has no nolune plan. Subscribe at ${base}`);
		expect(nolunePlanAccountUrl()).toBe(`${base}/`);
		expect(heard).toHaveBeenCalledTimes(1);

		// Pages ask again when they hear: the answer is kept a while, and no news isn't told.
		const asked = () => seen.filter((r) => r.path === '/v1/usage').length;
		const before = asked();
		for (let i = 0; i < 5; i++) expect(nolunePlanUsage()).toBeNull();
		expect((await nolunePlanStatus()).noPlan).toBe(true);
		await nolunePlanStatus({ check: true });
		expect(asked()).toBe(before + 1);
		expect(heard).toHaveBeenCalledTimes(1);

		// Subscribed on the account page.
		subscribed = true;
		expect(await nolunePlanStatus({ check: true })).toMatchObject({ noPlan: false, usage: USAGE });
		expect(heard).toHaveBeenCalledTimes(2);
		off();
	});

	it('says when background work may go on again, apart from people’s chats', () => {
		const at = new Date(Date.now() + 3 * 60 * 60 * 1000).toISOString();
		expect(planRefusal(429, { code: 'background_limit', resets_at: at })).toMatchObject({
			kind: 'background_limit',
			message: expect.stringMatching(
				/^Automations and other background work have spent what they may today on the nolune plan, to keep the rest of its credits for people's chats\. They start again (at|on) /
			)
		});
	});

	it('says when a limit starts again, in words', async () => {
		await link();
		const { user, chat } = planChat();
		const resetsAt = new Date(Date.now() + 2 * 60 * 60 * 1000);
		chats = [
			{
				status: 429,
				headers: { 'x-should-retry': 'false' },
				json: {
					error: {
						code: 'five_hour_limit',
						message: "nolune's 5-hour limit is reached.",
						resets_at: resetsAt.toISOString()
					}
				}
			}
		];
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Hi');
		await ended;

		const time = resetsAt.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
		// Two hours from late evening is tomorrow, which the message names.
		const day =
			resetsAt.toDateString() === new Date().toDateString()
				? ''
				: `on ${resetsAt.toLocaleDateString('en-GB', { weekday: 'long' })} `;
		expect(getSnapshot(chat.id).error).toContain(
			`The nolune plan's 5-hour limit is reached. Chats start again ${day}at ${time}.`
		);
		// Asked once: the SDK doesn't retry a refusal.
		expect(chatCalls()).toHaveLength(1);
		logged.mockRestore();
	});

	it('says how to link nolune when it isn’t, without asking the API', async () => {
		const { user, chat } = planChat();
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Hi');
		await ended;
		expect(getSnapshot(chat.id).error).toContain("nolune isn't linked to a nolune plan.");
		expect(chatCalls()).toHaveLength(0);
		logged.mockRestore();
	});

	it("lists the plan's models in its order, and uses the plan for memory search when there's no key", async () => {
		await link();
		expect(await listModels('nolune-plan')).toEqual([
			{
				id: 'anthropic/claude-haiku-4.5',
				name: 'Anthropic: Claude Haiku 4.5',
				description: null,
				contextWindow: 200_000
			},
			{ id: 'someone/newer-model', name: null, description: null, contextWindow: 100_000 }
		]);
		expect(embeddingSource()).toEqual({
			url: `${base}/v1`,
			model: 'openai/text-embedding-3-small',
			key: TOKEN,
			name: 'nolune-plan/openai/text-embedding-3-small'
		});
	});
});

describe('pictures on the nolune plan', () => {
	it('makes pictures on the plan when nolune has no key, fitted to the model', async () => {
		await link();
		expect(imageGenerationStatus()).toMatchObject({
			model: 'nolune-plan/openai/gpt-image-2.5-flare',
			ready: true
		});
		// The agent's command, in an automation's hidden chat.
		vi.stubEnv('NOLUNE_CONVERSATION_ID', 'chat-1');
		vi.stubEnv('NOLUNE_USE', 'background');
		const result = await generateImages({ prompt: 'a paper boat', size: 'portrait', count: 2 });

		expect(result.model).toBe('nolune-plan/openai/gpt-image-2.5-flare');
		expect(result.images.map((i) => [i.format, i.data.toString()])).toEqual([
			['png', 'a picture'],
			['png', 'a picture']
		]);
		// One picture per request is all this model makes: two requests.
		const asked = seen.filter((r) => r.path === '/v1/images');
		expect(asked.map((r) => r.json)).toEqual([
			{ model: 'openai/gpt-image-2.5-flare', prompt: 'a paper boat', aspect_ratio: '2:3' },
			{ model: 'openai/gpt-image-2.5-flare', prompt: 'a paper boat', aspect_ratio: '2:3' }
		]);
		expect(asked[0].headers).toMatchObject({
			authorization: `Bearer ${TOKEN}`,
			'x-nolune-use': 'background',
			'x-nolune-turn': 'continue'
		});
		vi.unstubAllEnvs();
	});

	it('says a limit in words, and what the model can’t do before asking', async () => {
		await link();
		pictures = [
			{
				status: 429,
				json: {
					error: { code: 'weekly_limit', message: 'Weekly limit.', resets_at: null }
				}
			}
		];
		await expect(generateImages({ prompt: 'a paper boat' })).rejects.toThrow(
			"The nolune plan's weekly limit is reached."
		);
		await expect(generateImages({ prompt: 'a paper boat', quality: 'max' })).rejects.toThrow(
			"openai/gpt-image-2.5-flare's quality is one of auto, low, medium, high."
		);
		await expect(
			generateImages({ prompt: 'a paper boat', model: 'nolune-plan/someone/else' })
		).rejects.toThrow('The nolune plan has no image model someone/else.');
	});
});
