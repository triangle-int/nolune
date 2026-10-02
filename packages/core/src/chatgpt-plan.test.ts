import { createHash, generateKeyPairSync, sign, type KeyObject } from 'node:crypto';
import { readFileSync, statSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import { chatGptPlanStatus, listChatGptModels } from './chatgpt-plan.ts';
import {
	activeRegistration,
	cancelChatGptSignIn,
	chatGptSignInState,
	finishChatGptSignIn,
	signOutChatGpt,
	startChatGptSignIn,
	type ChatGptSignIn
} from './chatgpt-sign-in.ts';
import { committedRows, createConversation, getConversation, setEffort } from './conversations.ts';
import type { Message } from './format.ts';
import { listModels } from './models.ts';
import { toResponsesInput } from './openai-chat.ts';
import { paths } from './paths.ts';
import { addPreset } from './presets.ts';
import { runCommand } from './run-command.ts';
import { getSnapshot, onLoopEnd, sendMessage } from './runner.ts';
import { makeFamily, makePreset, pdfWithPages, runCommandsUnchecked } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

/*
 * Chats on the ChatGPT plan against a stand-in for OpenAI: its accounts service (sign-in, token
 * refresh, sign-out, the keys that sign ID tokens) and the Responses API and model catalog the
 * plan's token opens, and for npm, which says Codex's latest release. The browser's part of a
 * sign-in is played by the tests.
 */

// --- a stand-in for OpenAI ---

interface Seen {
	method: string;
	path: string;
	query: URLSearchParams;
	headers: IncomingHttpHeaders;
	form: URLSearchParams | null;
	json: Record<string, unknown> | null;
}
type Item = Record<string, unknown>;
type Answer = {
	status?: number;
	headers?: Record<string, string>;
	json?: unknown;
	events?: object[];
};

let server: Server;
let base = '';
const seen: Seen[] = [];
/** The Responses API's answers. */
let answer: (request: Seen) => Answer;
/** The model catalog's answers. */
let modelsAnswer: (request: Seen) => Answer;
/** npm's answer for Codex's latest release. */
let codexRelease: () => Answer;
let lookups = 0;
/** The token endpoint's answer to a refresh. */
let refreshAnswer: (form: URLSearchParams) => Answer;

const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const KID = 'test-key';

/** What the sign-in page granted for a code, which the token endpoint gives tokens for. */
interface Grant {
	clientId: string;
	nonce: string;
	challenge: string;
	redirectUri: string;
	scope: string;
	subject: string;
	email: string;
}
const grants = new Map<string, Grant>();
let codes = 0;
let tokens = 0;

function jwt(claims: Record<string, unknown>, key: KeyObject = privateKey): string {
	const part = (value: object) => Buffer.from(JSON.stringify(value)).toString('base64url');
	const head = `${part({ alg: 'RS256', kid: KID, typ: 'JWT' })}.${part(claims)}`;
	return `${head}.${sign('RSA-SHA256', Buffer.from(head), key).toString('base64url')}`;
}

function idToken(grant: Grant, overrides: Record<string, unknown> = {}): string {
	const now = Math.floor(Date.now() / 1000);
	return jwt({
		iss: base,
		aud: grant.clientId,
		sub: grant.subject,
		email: grant.email,
		iat: now,
		exp: now + 3600,
		nonce: grant.nonce,
		'https://api.openai.com/auth': { chatgpt_plan_type: 'plus' },
		...overrides
	});
}

/** A token response, as the token endpoint sends one. */
function issued(scope: string, id?: string): Answer {
	tokens++;
	return {
		json: {
			access_token: `access_${tokens}`,
			refresh_token: `refresh_${tokens}`,
			...(id ? { id_token: id } : {}),
			token_type: 'Bearer',
			expires_in: 3600,
			scope
		}
	};
}

/** What the token endpoint does with an authorization code; a test may change the ID token. */
let exchangeIdToken: (grant: Grant) => string = (grant) => idToken(grant);

function tokenEndpoint(form: URLSearchParams): Answer {
	if (form.get('grant_type') === 'refresh_token') return refreshAnswer(form);
	const grant = grants.get(form.get('code') ?? '');
	const challenge = createHash('sha256')
		.update(form.get('code_verifier') ?? '')
		.digest('base64url');
	if (
		!grant ||
		grant.challenge !== challenge ||
		grant.redirectUri !== form.get('redirect_uri') ||
		grant.clientId !== form.get('client_id') ||
		form.get('resource') !== 'https://api.openai.com/v1'
	) {
		return { status: 400, json: { error: 'invalid_grant' } };
	}
	grants.delete(form.get('code')!);
	return issued(grant.scope, exchangeIdToken(grant));
}

const CATALOG = [
	{
		slug: 'gpt-6.1-sol',
		display_name: 'GPT-6.1 Sol',
		description: 'For hard problems.',
		visibility: 'list',
		minimal_client_version: '0.159.0',
		supported_reasoning_levels: [
			{ effort: 'low' },
			{ effort: 'medium' },
			{ effort: 'high' },
			{ effort: 'xhigh' }
		],
		context_window: 400_000
	},
	{ slug: 'gpt-6.1-luna', display_name: 'GPT-6.1 Luna', visibility: 'list' },
	{ slug: 'gpt-6.1-internal', display_name: 'Internal', visibility: 'hide' }
];

/**
 * The catalog as OpenAI answers it: a model only for a client at or past its
 * `minimal_client_version`, and without a version, an older list.
 */
function gatedCatalog(request: Seen): Answer {
	const asked = request.query.get('client_version') ?? '0.0.0';
	const offered = CATALOG.filter(
		(m) =>
			!m.minimal_client_version ||
			m.minimal_client_version.localeCompare(asked, 'en', { numeric: true }) <= 0
	);
	return { json: { models: offered } };
}

beforeAll(async () => {
	server = createServer(async (req, res) => {
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk as Buffer);
		const body = Buffer.concat(chunks).toString('utf8');
		const isForm = String(req.headers['content-type']).startsWith('application/x-www-form');
		let json: Seen['json'] = null;
		try {
			json = isForm ? null : (JSON.parse(body) as Seen['json']);
		} catch {
			// empty
		}
		const url = new URL(req.url ?? '', 'http://stand-in');
		const path = url.pathname;
		const request: Seen = {
			method: req.method ?? 'GET',
			path,
			query: url.searchParams,
			headers: req.headers,
			form: isForm ? new URLSearchParams(body) : null,
			json
		};
		seen.push(request);
		let reply: Answer;
		if (path === '/.well-known/jwks.json') {
			reply = {
				json: { keys: [{ ...publicKey.export({ format: 'jwk' }), kid: KID, alg: 'RS256' }] }
			};
		} else if (path === '/api/accounts/oauth/token') reply = tokenEndpoint(request.form!);
		else if (path === '/api/accounts/oauth/revoke') reply = { json: {} };
		else if (path === '/v1/models') reply = modelsAnswer(request);
		else if (path === '/codex/latest') reply = codexRelease();
		else if (path === '/v1/responses') reply = answer(request);
		else reply = { status: 404, json: { error: { message: 'Not found' } } };
		if (reply.events) {
			res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream', ...reply.headers });
			res.end(
				reply.events
					.map((e) => `event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`)
					.join('')
			);
		} else {
			res.writeHead(reply.status ?? 200, { 'content-type': 'application/json', ...reply.headers });
			res.end(JSON.stringify(reply.json ?? {}));
		}
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	vi.stubEnv('NOLUNE_CHATGPT_AUTH_URL', base);
	vi.stubEnv('NOLUNE_CHATGPT_API_URL', `${base}/v1`);
	// A new address each time, so each test asks npm afresh.
	vi.stubEnv('NOLUNE_CODEX_RELEASE_URL', `${base}/codex/latest?run=${++lookups}`);
	// The OpenAI key's settings, which the plan's requests never use.
	vi.stubEnv('OPENAI_BASE_URL', 'http://127.0.0.1:9/v1');
	vi.stubEnv('OPENAI_ORG_ID', 'org-of-the-key');
	// These are about the plan, not auto mode's checks (command-safety.test.ts).
	runCommandsUnchecked();
	seen.length = 0;
	grants.clear();
	codes = 0;
	tokens = 0;
	exchangeIdToken = (grant) => idToken(grant);
	answer = () => ({ status: 500, json: { error: { message: 'unexpected request' } } });
	modelsAnswer = gatedCatalog;
	codexRelease = () => ({ json: { name: '@openai/codex', version: '0.160.0' } });
	refreshAnswer = () =>
		issued('chatgpt.tokens.use.direct email offline_access openid profile resource.invoke');
});

afterEach(() => {
	cancelChatGptSignIn();
	vi.unstubAllEnvs();
	vi.resetAllMocks();
});

// --- the browser's part ---

/**
 * What OpenAI's sign-in page does once the person signed in and allowed nolune: the address it
 * sends the browser back to. A new registration gets `oaiapp_1`.
 */
function approve(
	signIn: ChatGptSignIn,
	opts: { scope?: string; subject?: string; state?: string } = {}
): URL {
	const q = new URL(signIn.url).searchParams;
	const registering = q.get('client_id') === 'dynamic_agent_client';
	const clientId = registering ? 'oaiapp_1' : q.get('client_id')!;
	const code = `code_${++codes}`;
	grants.set(code, {
		clientId,
		nonce: q.get('nonce')!,
		challenge: q.get('code_challenge')!,
		redirectUri: q.get('redirect_uri')!,
		scope: opts.scope ?? q.get('scope')!,
		subject: opts.subject ?? 'user_anna',
		email: 'anna@example.com'
	});
	const back = new URL(q.get('redirect_uri')!);
	back.search = new URLSearchParams({
		code,
		state: opts.state ?? q.get('state')!,
		...(registering ? { client_id: clientId } : {})
	}).toString();
	return back;
}

/** Signs Anna in, the browser coming back to this computer. */
async function signIn(): Promise<void> {
	const started = await startChatGptSignIn();
	await fetch(approve(started));
	await started.done;
}

function stored() {
	return JSON.parse(readFileSync(paths.chatgpt, 'utf8')) as {
		hostId: string;
		active: string | null;
		registrations: Record<string, unknown>[];
	};
}

/** Changes what's kept of the signed-in account, as time passing would. */
function change(fields: Record<string, unknown>): void {
	const s = stored();
	Object.assign(
		s.registrations.find((r) => r.clientId === s.active)!,
		fields
	);
	writeFileSync(paths.chatgpt, JSON.stringify(s));
}

function requests(path: string): Seen[] {
	return seen.filter((r) => r.path === path);
}

describe('signing in with ChatGPT', () => {
	it('registers nolune with the account and keeps the sign-in on this computer', async () => {
		const started = await startChatGptSignIn();
		const url = new URL(started.url);
		const q = url.searchParams;
		expect(`${url.origin}${url.pathname}`).toBe(`${base}/api/accounts/authorize`);
		expect(Object.fromEntries(q)).toEqual({
			client_id: 'dynamic_agent_client',
			agent_name_hint: 'nolune',
			ext_agent_host_id: expect.stringMatching(/^urn:uuid:[0-9a-f-]{36}$/),
			response_type: 'code',
			redirect_uri: expect.stringMatching(/^http:\/\/127\.0\.0\.1:\d+\/auth\/callback$/),
			scope: 'openid profile email offline_access resource.invoke chatgpt.tokens.use.direct',
			resource: 'https://api.openai.com/v1',
			state: expect.any(String),
			nonce: expect.any(String),
			code_challenge_method: 'S256',
			code_challenge: expect.any(String)
		});
		expect(chatGptSignInState().pending?.url).toBe(started.url);

		// The browser comes back to nolune's listener on this computer.
		const page = await fetch(approve(started));
		await started.done;
		expect(page.status).toBe(200);
		expect(await page.text()).toContain('Signed in with ChatGPT');

		const [exchange] = requests('/api/accounts/oauth/token');
		expect(Object.fromEntries(exchange.form!)).toEqual({
			grant_type: 'authorization_code',
			client_id: 'oaiapp_1',
			code: 'code_1',
			code_verifier: expect.any(String),
			redirect_uri: q.get('redirect_uri'),
			resource: 'https://api.openai.com/v1'
		});
		expect(statSync(paths.chatgpt).mode & 0o777).toBe(0o600);
		const s = stored();
		expect(s.hostId).toBe(q.get('ext_agent_host_id'));
		expect(s.active).toBe('oaiapp_1');
		expect(s.registrations).toEqual([
			expect.objectContaining({
				clientId: 'oaiapp_1',
				subject: 'user_anna',
				email: 'anna@example.com',
				plan: 'ChatGPT Plus',
				accessToken: expect.stringMatching(/^access_/),
				refreshToken: expect.stringMatching(/^refresh_/),
				idToken: expect.any(String)
			})
		]);
		expect(chatGptSignInState()).toEqual({ pending: null, signInError: null, previous: null });
		expect(await chatGptPlanStatus()).toEqual({
			path: null,
			installed: true,
			account: { email: 'anna@example.com', plan: 'ChatGPT Plus' },
			signedIn: 'signed in as anna@example.com (ChatGPT Plus)',
			problem: null
		});
	});

	it('finishes with the address a browser on another device ended on', async () => {
		const started = await startChatGptSignIn();
		const back = approve(started);
		// Another device shows the same address, which it couldn't open.
		back.port = '1';

		await finishChatGptSignIn(back.toString());
		await started.done;

		expect(activeRegistration()?.email).toBe('anna@example.com');
	});

	it('keeps waiting when the address pasted is from another sign-in', async () => {
		const started = await startChatGptSignIn();
		await expect(
			finishChatGptSignIn(approve(started, { state: 'older' }).toString())
		).rejects.toThrow(/another sign-in/);
		await expect(finishChatGptSignIn('https://example.com/')).rejects.toThrow(/127\.0\.0\.1/);
		expect(chatGptSignInState().pending).not.toBeNull();

		await finishChatGptSignIn(approve(started).toString());
		await started.done;
		expect(activeRegistration()).not.toBeNull();
	});

	it("turns down an ID token that isn't for this sign-in, and keeps nothing", async () => {
		exchangeIdToken = (grant) => idToken(grant, { nonce: 'from-elsewhere' });
		const started = await startChatGptSignIn();
		await fetch(approve(started));

		await expect(started.done).rejects.toThrow(/another sign-in/);
		expect(activeRegistration()).toBeNull();
		expect(chatGptSignInState().signInError).toMatch(/another sign-in/);
	});

	it('turns down an ID token signed with a key OpenAI does not list', async () => {
		const other = generateKeyPairSync('rsa', { modulusLength: 2048 }).privateKey;
		exchangeIdToken = (grant) => {
			const good = idToken(grant);
			const [head, claims] = good.split('.');
			const signature = sign('RSA-SHA256', Buffer.from(`${head}.${claims}`), other);
			return `${head}.${claims}.${signature.toString('base64url')}`;
		};
		const started = await startChatGptSignIn();
		await fetch(approve(started));

		await expect(started.done).rejects.toThrow(/bad signature/);
		expect(activeRegistration()).toBeNull();
	});

	it('signs in to the same account again with its registration', async () => {
		await signIn();
		const hostId = stored().hostId;

		const again = await startChatGptSignIn();
		const q = new URL(again.url).searchParams;
		expect(q.get('client_id')).toBe('oaiapp_1');
		expect(q.has('agent_name_hint')).toBe(false);
		expect(q.get('ext_agent_host_id')).toBe(hostId);
		expect(q.get('id_token_hint')).toEqual(expect.any(String));
		expect(q.get('login_hint')).toBe('anna@example.com');
		await fetch(approve(again));
		await again.done;

		expect(stored().registrations).toHaveLength(1);
		expect(activeRegistration()?.accessToken).toBe('access_2');
	});

	it("refuses another account's sign-in on an account's registration", async () => {
		await signIn();
		const again = await startChatGptSignIn();
		await fetch(approve(again, { subject: 'user_ben' }));

		await expect(again.done).rejects.toThrow(/another ChatGPT account/);
		expect(activeRegistration()?.subject).toBe('user_anna');
	});

	it("says so when the sign-in didn't allow the plan", async () => {
		const started = await startChatGptSignIn();
		await fetch(approve(started, { scope: 'openid profile email offline_access' }));
		await started.done;

		const status = await chatGptPlanStatus();
		expect(status.signedIn).toBe('signed in as anna@example.com (ChatGPT Plus)');
		expect(status.problem).toMatch(/without letting nolune use its ChatGPT plan/);
		await expect(addPreset({ provider: 'chatgpt-plan', model: 'gpt-6.1-sol' })).rejects.toThrow(
			/Sign in again and allow it/
		);
		// Asked again the next time.
		const again = await startChatGptSignIn();
		expect(new URL(again.url).searchParams.get('prompt')).toBe('consent');
	});

	it('signs out, telling OpenAI, and keeps the registration for next time', async () => {
		await signIn();
		const { refreshToken } = activeRegistration()!;

		expect(await signOutChatGpt()).toBe(true);

		const [revoke] = requests('/api/accounts/oauth/revoke');
		expect(Object.fromEntries(revoke.form!)).toEqual({
			token: refreshToken,
			token_type_hint: 'refresh_token',
			client_id: 'oaiapp_1'
		});
		expect(activeRegistration()).toBeNull();
		expect(stored().registrations[0]).toMatchObject({
			clientId: 'oaiapp_1',
			accessToken: null,
			refreshToken: null
		});
		expect(chatGptSignInState().previous).toBe('anna@example.com');
		expect((await chatGptPlanStatus()).problem).toMatch(/Nobody is signed in with ChatGPT/);
		// Signing in again reuses it, without the ID token that was forgotten.
		const again = await startChatGptSignIn();
		const q = new URL(again.url).searchParams;
		expect(q.get('client_id')).toBe('oaiapp_1');
		expect(q.has('id_token_hint')).toBe(false);
		// Or registers with another account.
		const other = await startChatGptSignIn({ anotherAccount: true });
		expect(new URL(other.url).searchParams.get('client_id')).toBe('dynamic_agent_client');
	});

	it('refreshes the access token before it runs out', async () => {
		await signIn();
		change({ expiresAt: Date.now() + 60_000 });

		expect((await listChatGptModels()).map((m) => m.id)).toContain('gpt-6.1-sol');

		const [refresh] = requests('/api/accounts/oauth/token').slice(1);
		expect(Object.fromEntries(refresh.form!)).toEqual({
			grant_type: 'refresh_token',
			client_id: 'oaiapp_1',
			refresh_token: 'refresh_1',
			resource: 'https://api.openai.com/v1'
		});
		expect(requests('/v1/models')[0].headers.authorization).toBe('Bearer access_2');
		expect(activeRegistration()).toMatchObject({
			accessToken: 'access_2',
			refreshToken: 'refresh_2'
		});
	});

	it('ends the sign-in when its refresh token no longer works', async () => {
		await signIn();
		change({ expiresAt: Date.now() - 1000 });
		refreshAnswer = () => ({ status: 400, json: { error: 'refresh_token_reused' } });

		await expect(listChatGptModels()).rejects.toThrow(/sign-in has ended \(refresh_token_reused\)/);
		expect(activeRegistration()).toMatchObject({ accessToken: null, refreshToken: null });
		expect((await chatGptPlanStatus()).signedIn).toBeNull();
	});
});

// --- chats ---

let ids = 0;

function said(text: string): Item {
	return {
		id: `msg_${++ids}`,
		type: 'message',
		role: 'assistant',
		status: 'completed',
		content: [{ type: 'output_text', text, annotations: [] }]
	};
}

function thought(text: string): Item {
	return {
		id: `rs_${++ids}`,
		type: 'reasoning',
		summary: [{ type: 'summary_text', text }],
		encrypted_content: 'gAAAA-encrypted'
	};
}

/** A call of nolune's run_command, in nolune's namespace as the plan calls it. */
function runs(command: string): Item {
	return {
		id: `fc_${++ids}`,
		type: 'function_call',
		status: 'completed',
		call_id: `call_${ids}`,
		namespace: 'nolune',
		name: 'run_command',
		arguments: JSON.stringify({ summary: 'Listing the files', icon: 'folder-open', command })
	};
}

/**
 * A streamed response that sends `output` item by item, as the Responses API does. `bare`: its
 * last event leaves the output out, as the plan's route does, with only the items' own events.
 */
function streamed(
	output: Item[],
	failure?: { code: string; message: string },
	opts: { bare?: boolean } = {}
): Answer {
	const events: object[] = [{ type: 'response.created', response: { status: 'in_progress' } }];
	output.forEach((item, index) => {
		events.push({ type: 'response.output_item.added', output_index: index, item });
		if (item.type === 'message') {
			for (const part of item.content as { text: string }[]) {
				events.push({ type: 'response.output_text.delta', output_index: index, delta: part.text });
			}
		}
		events.push({ type: 'response.output_item.done', output_index: index, item });
	});
	events.push(
		failure
			? { type: 'response.failed', response: { status: 'failed', error: failure, output } }
			: {
					type: 'response.completed',
					response: {
						status: 'completed',
						output: opts.bare ? [] : output,
						usage: {
							input_tokens: 1000,
							input_tokens_details: { cached_tokens: 800 },
							output_tokens: 40
						}
					}
				}
	);
	return { events };
}

function isTitleRequest(request: Seen): boolean {
	return String(request.json?.instructions ?? '').includes('You name chats');
}

/** Answers the chat's own requests with `replies` in turn, and title requests with a title. */
function scripted(...replies: Answer[]): void {
	let next = 0;
	answer = (request) => {
		if (isTitleRequest(request)) return streamed([said('Files here')]);
		return replies[next++] ?? { status: 500, json: { error: { message: 'no more replies' } } };
	};
}

function chatCalls(): Seen[] {
	return requests('/v1/responses').filter((r) => !isTitleRequest(r));
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
	const { user, profile } = makeFamily();
	const preset = makePreset('ChatGPT', 'gpt-6.1-sol', 'chatgpt-plan');
	const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
	return { user, profile, chat };
}

function rowsOf(conversationId: string) {
	return committedRows(conversationId).map((row) => ({
		kind: row.kind,
		content: JSON.parse(row.content) as unknown
	}));
}

const DOT =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';

/** What a request may not carry on the plan (developers.openai.com/siwc, Preview limitations). */
const UNSUPPORTED = [
	'background',
	'conversation',
	'max_output_tokens',
	'max_tool_calls',
	'metadata',
	'previous_response_id',
	'prompt',
	'safety_identifier',
	'temperature',
	'top_p',
	'truncation',
	'user'
];

describe('chats on the ChatGPT plan', () => {
	it("runs a turn in nolune's own loop, on the plan's token", async () => {
		await signIn();
		const { user, chat } = planChat();
		// Above what the model takes: the nearest below it is sent.
		setEffort(chat.id, 'max');
		scripted(
			streamed([thought('Listing them.'), runs('ls')]),
			streamed([said('One file: a.txt.')])
		);
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Files?');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		expect(rowsOf(chat.id).map((row) => row.kind)).toEqual([
			'human',
			'assistant',
			'tool_results',
			'assistant'
		]);
		expect(vi.mocked(runCommand).mock.calls[0][0]).toMatchObject({ command: 'ls' });
		// Each model call's use, as nolune's own loop saves it.
		const replies = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(replies.map((row) => row.stopReason)).toEqual(['tool_use', 'end_turn']);
		for (const row of replies) {
			expect(JSON.parse(row.usage ?? '{}')).toEqual({
				input: 200,
				cacheRead: 800,
				cacheWrite: 0,
				output: 40
			});
		}

		const [first, second] = chatCalls();
		expect(first.headers.authorization).toBe('Bearer access_1');
		// Never the OpenAI key's organization.
		expect(first.headers['openai-organization']).toBeUndefined();
		expect(first.json).toMatchObject({
			model: 'gpt-6.1-sol',
			instructions: chat.systemPrompt,
			store: false,
			stream: true,
			reasoning: { effort: 'xhigh', summary: 'auto' },
			include: ['reasoning.encrypted_content'],
			prompt_cache_key: chat.id,
			tools: [
				{
					type: 'namespace',
					name: 'nolune',
					description: "nolune's tools.",
					tools: [expect.objectContaining({ type: 'function', name: 'run_command' })]
				}
			]
		});
		for (const field of UNSUPPORTED) expect(first.json).not.toHaveProperty(field);
		expect(Array.isArray(first.json?.input)).toBe(true);
		// The whole transcript again, with the call in nolune's namespace and its result.
		const input = second.json?.input as Item[];
		expect(input).toContainEqual(expect.objectContaining({ type: 'reasoning', id: 'rs_1' }));
		expect(input).toContainEqual(
			expect.objectContaining({ type: 'function_call', namespace: 'nolune', name: 'run_command' })
		);
		expect(input).toContainEqual(
			expect.objectContaining({ type: 'function_call_output', output: 'a.txt\n[exit code 0]' })
		);

		// Named on the plan too, streamed and without a cap on its length.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Files here'));
		const title = requests('/v1/responses').find(isTitleRequest)!;
		expect(title.json).toMatchObject({ stream: true, store: false });
		expect(title.json).not.toHaveProperty('max_output_tokens');
		expect(Array.isArray(title.json?.input)).toBe(true);
	});

	it('sends pictures and PDFs inline, having no Files API', async () => {
		await signIn();
		const { user, profile, chat } = planChat();
		const photo = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'dot.png',
			body: Readable.from([Buffer.from(DOT, 'base64')])
		});
		const menu = pdfWithPages(1);
		const pdf = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'menu.pdf',
			body: Readable.from([menu])
		});
		scripted(streamed([said('A dot and a menu.')]));

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'What are these?', [
			photo.id,
			pdf.id
		]);
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		const message = (chatCalls()[0].json?.input as Item[]).findLast((i) => i.role === 'user');
		expect(message?.content).toEqual([
			{ type: 'input_text', text: expect.stringMatching(/^\[Anna attached dot\.png, saved at /) },
			{ type: 'input_image', image_url: `data:image/png;base64,${DOT}`, detail: 'auto' },
			{ type: 'input_text', text: expect.stringMatching(/^\[Anna attached menu\.pdf, saved at /) },
			{
				type: 'input_file',
				filename: 'menu.pdf',
				file_data: `data:application/pdf;base64,${menu.toString('base64')}`
			},
			{ type: 'input_text', text: 'Anna: What are these?' }
		]);
	});

	it("keeps the reply when the stream's last event leaves its output out", async () => {
		await signIn();
		const { user, chat } = planChat();
		answer = (request) =>
			isTitleRequest(request)
				? streamed([said('Disk space')], undefined, { bare: true })
				: chatCalls().length === 1
					? streamed([thought('Checking the disk.'), runs('df -h /')], undefined, { bare: true })
					: streamed([said('About 120 GB are free.')], undefined, { bare: true });
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: '/dev/disk1 500G 380G 120G\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'How much disk is free?');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		expect(vi.mocked(runCommand).mock.calls[0][0]).toMatchObject({ command: 'df -h /' });
		const replies = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(replies.map((row) => row.stopReason)).toEqual(['tool_use', 'end_turn']);
		expect(replies[1].content).toContain('About 120 GB are free.');
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Disk space'));
	});

	it("says where to look when the plan's limit is reached", async () => {
		await signIn();
		const { user, chat } = planChat();
		scripted(
			streamed([], {
				code: 'subscription_sharing_usage_limit_exceeded',
				message: 'Usage limit reached.'
			})
		);
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Hi');
		await ended;

		expect(getSnapshot(chat.id).error).toMatch(
			/usage limit is reached.*https:\/\/chatgpt\.com\/settings\/usage/
		);
		logged.mockRestore();
	});

	it("says so when the account can't use its plan in other apps", async () => {
		await signIn();
		const { user, chat } = planChat();
		answer = () => ({
			status: 403,
			json: {
				error: {
					code: 'subscription_sharing_user_not_eligible',
					message: 'Not eligible.',
					type: 'invalid_request_error'
				}
			}
		});
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Hi');
		await ended;

		expect(getSnapshot(chat.id).error).toBe(
			"anna@example.com can't use its ChatGPT plan in other apps: that takes ChatGPT Plus or Pro."
		);
		logged.mockRestore();
	});

	it('says how to sign in when nobody is, without asking OpenAI', async () => {
		const { user, chat } = planChat();
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Hi');
		await ended;

		expect(getSnapshot(chat.id).error).toMatch(
			/Nobody is signed in with ChatGPT.*nolune chatgpt-plan setup/
		);
		expect(requests('/v1/responses')).toEqual([]);
		logged.mockRestore();
	});

	it('sends replies from when Codex ran the plan as the plan takes them', () => {
		const messages: Message[] = [
			{ role: 'user', blocks: [{ type: 'text', text: 'Anna: Files?' }] },
			{
				role: 'assistant',
				blocks: [],
				native: {
					provider: 'chatgpt-plan',
					model: 'gpt-6.1-sol',
					content: [
						// Codex's summary without encrypted reasoning, which can't go back.
						{ id: 'item_1', type: 'reasoning', summary: [{ type: 'summary_text', text: 'Hm.' }] },
						{
							id: 'item_2',
							type: 'message',
							role: 'assistant',
							status: 'completed',
							content: [{ type: 'output_text', text: 'Listing.', annotations: [] }]
						},
						{
							type: 'function_call',
							call_id: 'call_9',
							name: 'run_command',
							arguments: '{"command":"ls"}',
							status: 'completed'
						}
					]
				}
			}
		];

		expect(toResponsesInput(messages, 'gpt-6.1-sol', 'chatgpt-plan').slice(1)).toEqual([
			{ role: 'assistant', content: 'Listing.' },
			{
				type: 'function_call',
				call_id: 'call_9',
				name: 'run_command',
				arguments: '{"command":"ls"}',
				status: 'completed',
				namespace: 'nolune'
			}
		]);
	});

	it("lists the plan's models, and checks the model when a preset is added", async () => {
		await signIn();

		expect(await listModels('chatgpt-plan')).toEqual([
			{
				id: 'gpt-6.1-sol',
				name: 'GPT-6.1 Sol',
				description: 'For hard problems.',
				contextWindow: 400_000
			},
			{ id: 'gpt-6.1-luna', name: 'GPT-6.1 Luna', description: null, contextWindow: null }
		]);
		const preset = await addPreset({ provider: 'chatgpt-plan', model: 'gpt-6.1-sol' });
		expect(preset.modelContextWindow).toBe(400_000);
		await expect(addPreset({ provider: 'chatgpt-plan', model: 'gpt-5' })).rejects.toThrow(
			'The ChatGPT plan has no model "gpt-5". It has gpt-6.1-sol, gpt-6.1-luna.'
		);
	});

	it("asks for the plan's models as Codex's latest release, which new models need", async () => {
		await signIn();
		codexRelease = () => ({ json: { name: '@openai/codex', version: '0.161.2' } });

		expect((await listChatGptModels()).map((m) => m.id)).toContain('gpt-6.1-sol');
		await listChatGptModels();

		expect(requests('/v1/models').map((r) => r.query.get('client_version'))).toEqual([
			'0.161.2',
			'0.161.2'
		]);
		// npm is asked once an hour at most.
		expect(requests('/codex/latest')).toHaveLength(1);
	});

	it("asks as the Codex nolune knows when npm doesn't say a newer one", async () => {
		await signIn();
		codexRelease = () => ({ status: 503, json: {} });
		expect((await listChatGptModels()).map((m) => m.id)).toContain('gpt-6.1-sol');

		vi.stubEnv('NOLUNE_CODEX_RELEASE_URL', `${base}/codex/latest?older`);
		codexRelease = () => ({ json: { name: '@openai/codex', version: '0.150.0' } });
		await listChatGptModels();

		vi.stubEnv('NOLUNE_CODEX_RELEASE_URL', `${base}/codex/latest?alpha`);
		codexRelease = () => ({ json: { name: '@openai/codex', version: '0.170.0-alpha.1' } });
		await listChatGptModels();

		expect(requests('/v1/models').map((r) => r.query.get('client_version'))).toEqual([
			'0.160.0',
			'0.160.0',
			'0.160.0'
		]);
	});

	it("lists the plan's models without a version when OpenAI doesn't take one", async () => {
		await signIn();
		modelsAnswer = (request) =>
			request.query.has('client_version')
				? { status: 400, json: { error: { message: 'Unknown parameter: client_version' } } }
				: gatedCatalog(request);

		expect((await listChatGptModels()).map((m) => m.id)).toEqual([
			'gpt-6.1-luna',
			'gpt-6.1-internal'
		]);
		expect(requests('/v1/models').map((r) => r.query.has('client_version'))).toEqual([true, false]);
	});
});
