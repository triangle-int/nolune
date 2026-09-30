import {
	createHash,
	createPublicKey,
	randomBytes,
	randomUUID,
	verify,
	type JsonWebKey
} from 'node:crypto';
import {
	chmodSync,
	mkdirSync,
	openSync,
	closeSync,
	readFileSync,
	renameSync,
	rmSync,
	statSync,
	writeFileSync
} from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { dirname } from 'node:path';
import { paths } from './paths.ts';
import { PlanError, type PlanAccount } from './plans.ts';

/*
 * Signing in with ChatGPT, for chats on the ChatGPT plan (chatgpt-plan.ts): OpenAI's Sign in with
 * ChatGPT for open-source, locally hosted apps (developers.openai.com/siwc). nolune registers as
 * an app of the person's ChatGPT account, which lets it send Responses API requests that count
 * toward their Plus or Pro plan, and keeps what it gets on this computer.
 *
 * - **Sign-in.** OAuth with PKCE in a browser, which OpenAI sends back to a listener on
 *   127.0.0.1, the only address it takes: this computer. A browser on another device ends on a
 *   page that doesn't load, whose address can be pasted into nolune instead (finishChatGptSignIn).
 *   The first sign-in of an account registers nolune with it (`dynamic_agent_client`), which
 *   issues a client id of its own; later sign-ins to that account reuse it.
 * - **Host.** This computer has an id of its own (`ext_agent_host_id`), made once and kept, which
 *   OpenAI uses to tell hosts of the same app apart. It identifies nothing else.
 * - **Credentials.** In `~/.nolune/chatgpt.json`, readable by this user only: each account's
 *   client id and identity, and the signed-in one's tokens. The access token lasts an hour and
 *   is refreshed before it runs out; each refresh replaces the refresh token too, so refreshes
 *   are made one at a time, across processes (the gateway, and `nolune` in a terminal).
 */

const DEFAULT_AUTH_URL = 'https://auth.openai.com';
/** What the tokens are for, and the access token's audience. */
export const API_RESOURCE = 'https://api.openai.com/v1';
/** The scope that lets nolune use the plan: without it the sign-in is only who someone is. */
export const PLAN_SCOPE = 'chatgpt.tokens.use.direct';
const SCOPES = ['openid', 'profile', 'email', 'offline_access', 'resource.invoke', PLAN_SCOPE];
/** The name OpenAI shows for nolune when an account registers it; the person can change it. */
const APP_NAME = 'nolune';
const CALLBACK_PATH = '/auth/callback';

/** How long a sign-in waits for the browser to come back. */
const SIGN_IN_TIMEOUT_MS = 10 * 60 * 1000;
const REQUEST_TIMEOUT_MS = 30_000;
/** Access tokens are refreshed this long before they run out. */
const REFRESH_MARGIN_MS = 5 * 60 * 1000;
/** Tokens a little early or late by this computer's clock still count. */
const CLOCK_SKEW_MS = 5_000;
/** A refresh lock older than this was left by a process that ended. */
const STALE_LOCK_MS = 60_000;
const LOCK_WAIT_MS = 20_000;

export const CHATGPT_SIGN_IN_HELP =
	'Sign in with ChatGPT under Models & keys on the admin page, or run `nolune chatgpt-plan setup` on the computer nolune runs on.';
export const CHATGPT_USAGE_URL = 'https://chatgpt.com/settings/usage';

/** OpenAI's accounts service. NOLUNE_CHATGPT_AUTH_URL points it at a stand-in, for tests. */
function authUrl(): string {
	return (process.env.NOLUNE_CHATGPT_AUTH_URL || DEFAULT_AUTH_URL).replace(/\/+$/, '');
}

const endpoints = () => {
	const base = authUrl();
	return {
		issuer: base,
		authorize: `${base}/api/accounts/authorize`,
		token: `${base}/api/accounts/oauth/token`,
		revoke: `${base}/api/accounts/oauth/revoke`,
		jwks: `${base}/.well-known/jwks.json`
	};
};

// --- what's kept ---

/** An account nolune is registered with. Its tokens are null once it's signed out. */
export interface Registration {
	/** Issued by OpenAI when the account registered nolune (`oaiapp_…`). */
	clientId: string;
	/** Who signed in, from the validated ID token. */
	subject: string;
	email: string | null;
	/** The plan as ChatGPT names it ("ChatGPT Plus"), when the ID token says. */
	plan: string | null;
	idToken: string | null;
	accessToken: string | null;
	refreshToken: string | null;
	/** When the access token runs out, and the earliest OpenAI wants it refreshed (ms). */
	expiresAt: number | null;
	earliestRefreshAt: number | null;
	scopes: string[];
	savedAt: string;
}

interface Stored {
	/** This computer's `ext_agent_host_id`. */
	hostId: string;
	/** The client id of the account that's signed in, if any. */
	active: string | null;
	registrations: Registration[];
}

function readStored(): Stored | null {
	try {
		const stored = JSON.parse(readFileSync(paths.chatgpt, 'utf8')) as Partial<Stored>;
		if (typeof stored.hostId !== 'string') return null;
		return {
			hostId: stored.hostId,
			active: typeof stored.active === 'string' ? stored.active : null,
			registrations: Array.isArray(stored.registrations) ? stored.registrations : []
		};
	} catch {
		return null;
	}
}

/** Written whole and renamed into place, so another process never reads half of it. */
function writeStored(stored: Stored): void {
	mkdirSync(dirname(paths.chatgpt), { recursive: true });
	const temp = `${paths.chatgpt}.${process.pid}.tmp`;
	writeFileSync(temp, JSON.stringify(stored, null, '\t') + '\n', { mode: 0o600 });
	chmodSync(temp, 0o600);
	renameSync(temp, paths.chatgpt);
}

/** What's kept, with this computer's host id made the first time. */
function stored(): Stored {
	const found = readStored();
	if (found) return found;
	const made: Stored = { hostId: `urn:uuid:${randomUUID()}`, active: null, registrations: [] };
	writeStored(made);
	return made;
}

function activeOf(s: Stored | null): Registration | null {
	return s?.registrations.find((r) => r.clientId === s.active) ?? null;
}

/** The account that's signed in, or null. */
export function activeRegistration(): Registration | null {
	return activeOf(readStored());
}

/** The account signed in last, still registered but signed out, for signing in to again. */
function lastRegistration(s: Stored | null): Registration | null {
	return activeOf(s) ?? s?.registrations.at(-1) ?? null;
}

export function planAccountOf(r: Registration): PlanAccount {
	return { email: r.email, plan: r.plan };
}

/** Runs `work` while this process alone may change the tokens, other processes included. */
async function withLock<T>(work: () => Promise<T>): Promise<T> {
	const lock = `${paths.chatgpt}.lock`;
	mkdirSync(dirname(lock), { recursive: true });
	const until = Date.now() + LOCK_WAIT_MS;
	for (;;) {
		try {
			closeSync(openSync(lock, 'wx', 0o600));
			break;
		} catch (err) {
			if ((err as NodeJS.ErrnoException).code !== 'EEXIST') throw err;
			try {
				if (Date.now() - statSync(lock).mtimeMs > STALE_LOCK_MS) rmSync(lock, { force: true });
			} catch {
				// gone already
			}
			if (Date.now() > until)
				throw new PlanError('Another nolune is refreshing the ChatGPT sign-in.');
			await new Promise((resolve) => setTimeout(resolve, 100));
		}
	}
	try {
		return await work();
	} finally {
		rmSync(lock, { force: true });
	}
}

// --- OpenAI's accounts service ---

interface TokenResponse {
	access_token?: string;
	refresh_token?: string;
	id_token?: string;
	expires_in?: number;
	scope?: string;
	earliest_refresh_at?: number | string;
}

/** An error from the accounts service, with its OAuth `error` code when it gave one. */
class AuthServiceError extends Error {
	readonly status: number | null;
	readonly code: string | null;
	constructor(message: string, status: number | null, code: string | null) {
		super(message);
		this.status = status;
		this.code = code;
	}
}

async function postForm(url: string, form: Record<string, string>): Promise<TokenResponse> {
	let res: Response;
	try {
		res = await fetch(url, {
			method: 'POST',
			headers: {
				accept: 'application/json',
				'content-type': 'application/x-www-form-urlencoded'
			},
			body: new URLSearchParams(form),
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
		});
	} catch (err) {
		const why = err instanceof Error ? err.message : String(err);
		throw new AuthServiceError(`Couldn't reach ChatGPT's sign-in service (${why}).`, null, null);
	}
	const text = await res.text();
	let body: Record<string, unknown> = {};
	try {
		body = text ? (JSON.parse(text) as Record<string, unknown>) : {};
	} catch {
		// not JSON
	}
	if (!res.ok) {
		const code = typeof body.error === 'string' ? body.error : null;
		const detail =
			(typeof body.error_description === 'string' && body.error_description) ||
			code ||
			text.slice(0, 200) ||
			res.statusText;
		throw new AuthServiceError(
			`ChatGPT's sign-in service said ${res.status}: ${detail}`,
			res.status,
			code
		);
	}
	return body as TokenResponse;
}

/** A timestamp in seconds or milliseconds, as ms. */
function toMs(value: unknown): number | null {
	const n = typeof value === 'string' ? Number(value) : value;
	if (typeof n !== 'number' || !Number.isFinite(n) || n <= 0) return null;
	return n < 1e12 ? n * 1000 : n;
}

/** The token response's tokens, as a registration keeps them. */
function tokensOf(
	response: TokenResponse,
	previous: Pick<Registration, 'idToken' | 'refreshToken' | 'scopes'> | null
) {
	if (typeof response.access_token !== 'string' || !response.access_token) {
		throw new PlanError("ChatGPT's sign-in service sent no access token.");
	}
	const now = Date.now();
	return {
		accessToken: response.access_token,
		refreshToken: response.refresh_token || previous?.refreshToken || null,
		idToken: response.id_token || previous?.idToken || null,
		expiresAt: now + (response.expires_in ?? 3600) * 1000,
		earliestRefreshAt: toMs(response.earliest_refresh_at),
		scopes: response.scope ? response.scope.split(/\s+/).filter(Boolean) : (previous?.scopes ?? []),
		savedAt: new Date(now).toISOString()
	};
}

// --- the ID token ---

interface Jwk {
	kid?: string;
	kty: string;
	[key: string]: unknown;
}

let jwksCache: { url: string; keys: Jwk[] } | null = null;

async function jwks(refresh: boolean): Promise<Jwk[]> {
	const url = endpoints().jwks;
	if (!refresh && jwksCache?.url === url) return jwksCache.keys;
	const res = await fetch(url, { signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS) });
	if (!res.ok) throw new PlanError(`Couldn't get ChatGPT's signing keys (${res.status}).`);
	const keys = ((await res.json()) as { keys?: Jwk[] }).keys ?? [];
	jwksCache = { url, keys };
	return keys;
}

function decodePart(part: string): Record<string, unknown> {
	return JSON.parse(Buffer.from(part, 'base64url').toString('utf8')) as Record<string, unknown>;
}

const PLAN_NAMES: Record<string, string> = {
	free: 'ChatGPT Free',
	go: 'ChatGPT Go',
	plus: 'ChatGPT Plus',
	pro: 'ChatGPT Pro',
	prolite: 'ChatGPT Pro',
	team: 'ChatGPT Business',
	business: 'ChatGPT Business',
	enterprise: 'ChatGPT Enterprise',
	edu: 'ChatGPT Edu'
};

/** The plan as ChatGPT names it, from its word for it ("plus", "self_serve_business_…"). */
export function planName(planType: unknown): string | null {
	if (typeof planType !== 'string' || !planType || planType === 'unknown') return null;
	const exact = PLAN_NAMES[planType];
	if (exact) return exact;
	const family = Object.keys(PLAN_NAMES).find((key) => planType.split('_').includes(key));
	if (family) return PLAN_NAMES[family];
	return `ChatGPT (${planType})`;
}

interface Identity {
	subject: string;
	email: string | null;
	plan: string | null;
}

/**
 * Checks the ID token as OpenID Connect says: signed by one of OpenAI's keys, issued by it, for
 * this client, not expired, and for this sign-in (`nonce`). Returns who signed in.
 */
async function verifyIdToken(
	idToken: string,
	expect: { clientId: string; nonce: string }
): Promise<Identity> {
	const parts = idToken.split('.');
	if (parts.length !== 3) throw new PlanError("ChatGPT's ID token isn't a JWT.");
	let header: Record<string, unknown>;
	let claims: Record<string, unknown>;
	try {
		header = decodePart(parts[0]);
		claims = decodePart(parts[1]);
	} catch {
		throw new PlanError("ChatGPT's ID token couldn't be read.");
	}
	if (header.alg !== 'RS256') {
		throw new PlanError(`ChatGPT's ID token is signed with ${String(header.alg)}, not RS256.`);
	}
	const find = (keys: Jwk[]) =>
		keys.find((k) => k.kty === 'RSA' && (header.kid === undefined || k.kid === header.kid));
	// A key OpenAI started using since the keys were fetched.
	const jwk = find(await jwks(false)) ?? find(await jwks(true));
	if (!jwk) throw new PlanError("ChatGPT's ID token is signed with a key OpenAI doesn't list.");
	const signed = verify(
		'RSA-SHA256',
		Buffer.from(`${parts[0]}.${parts[1]}`),
		createPublicKey({ key: jwk as JsonWebKey, format: 'jwk' }),
		Buffer.from(parts[2], 'base64url')
	);
	if (!signed) throw new PlanError("ChatGPT's ID token has a bad signature.");
	const audiences = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
	const now = Date.now();
	if (claims.iss !== endpoints().issuer) {
		throw new PlanError(`ChatGPT's ID token comes from ${String(claims.iss)}.`);
	}
	if (!audiences.includes(expect.clientId)) {
		throw new PlanError("ChatGPT's ID token is for another app.");
	}
	if (typeof claims.exp !== 'number' || claims.exp * 1000 < now - CLOCK_SKEW_MS) {
		throw new PlanError("ChatGPT's ID token has run out. Is this computer's clock right?");
	}
	if (typeof claims.iat !== 'number') throw new PlanError("ChatGPT's ID token has no issue time.");
	if (claims.nonce !== expect.nonce) {
		throw new PlanError("ChatGPT's ID token is from another sign-in.");
	}
	if (typeof claims.sub !== 'string' || !claims.sub) {
		throw new PlanError("ChatGPT's ID token doesn't say who signed in.");
	}
	const auth = claims['https://api.openai.com/auth'] as Record<string, unknown> | undefined;
	return {
		subject: claims.sub,
		email: typeof claims.email === 'string' && claims.email ? claims.email : null,
		plan: planName(auth?.chatgpt_plan_type)
	};
}

// --- signing in ---

export interface ChatGptSignIn {
	/** OpenAI's sign-in page, to open in a browser. */
	url: string;
	/** When nolune stops waiting for the browser to come back, in ms since the epoch. */
	expiresAt: number;
	/** Resolves once signed in; rejects when the sign-in doesn't finish. */
	done: Promise<void>;
}

interface Attempt {
	signIn: ChatGptSignIn;
	/** Takes the address the browser came back to, from the listener or pasted. */
	complete: (url: URL) => Promise<void>;
	/** Ends the sign-in, once: with an error unless it's done, said in `lastError` unless quiet. */
	finish: (err: Error | null, quiet?: boolean) => void;
}

/** The sign-in under way in this process. */
let pending: Attempt | null = null;
/** Why the last sign-in in this process didn't finish. */
let lastError: string | null = null;

function randomValue(bytes = 32): string {
	return randomBytes(bytes).toString('base64url');
}

function listen(server: Server): Promise<number> {
	return new Promise((resolve, reject) => {
		server.once('error', reject);
		server.listen(0, '127.0.0.1', () => {
			server.off('error', reject);
			resolve((server.address() as AddressInfo).port);
		});
	});
}

function page(title: string, text: string): string {
	const escape = (s: string) =>
		s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);
	return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>nolune</title><style>body{font:16px/1.5 system-ui,sans-serif;max-width:32rem;margin:15vh auto;padding:0 1rem;color:#222;background:#fff}@media(prefers-color-scheme:dark){body{color:#eee;background:#111}}</style></head><body><h1>${escape(title)}</h1><p>${escape(text)}</p></body></html>`;
}

/**
 * Starts signing in with ChatGPT: returns OpenAI's sign-in page to open, and waits in the
 * background for the browser to come back to this computer, or for the address it ended on
 * (finishChatGptSignIn). Replaces a sign-in already under way. `anotherAccount` registers nolune
 * with an account it doesn't know yet; otherwise the account signed in last signs in again.
 */
export async function startChatGptSignIn(
	opts: { anotherAccount?: boolean } = {}
): Promise<ChatGptSignIn> {
	cancelChatGptSignIn();
	lastError = null;
	const s = stored();
	const again = opts.anotherAccount ? null : lastRegistration(s);
	const clientId = again?.clientId ?? 'dynamic_agent_client';
	const state = randomValue();
	const nonce = randomValue();
	const verifier = randomValue(48);
	const challenge = createHash('sha256').update(verifier).digest('base64url');

	let settle!: { resolve: () => void; reject: (err: Error) => void };
	const done = new Promise<void>((resolve, reject) => (settle = { resolve, reject }));
	// Callers that never wait for it mustn't turn its rejection into an unhandled one.
	done.catch(() => {});

	let completing: Promise<void> | null = null;
	let finished = false;
	const server = createServer((req, res) => {
		const url = new URL(req.url ?? '/', 'http://127.0.0.1');
		if (req.method !== 'GET' || url.pathname !== CALLBACK_PATH) {
			res.writeHead(404, { 'content-type': 'text/plain', connection: 'close' }).end('Not found');
			return;
		}
		attempt.complete(url).then(
			() =>
				res
					.writeHead(200, { 'content-type': 'text/html; charset=utf-8', connection: 'close' })
					.end(
						page(
							'Signed in with ChatGPT',
							'nolune can use your ChatGPT plan now. You can close this tab.'
						)
					),
			(err: Error) =>
				res
					.writeHead(400, { 'content-type': 'text/html; charset=utf-8', connection: 'close' })
					.end(page("The sign-in didn't finish", err.message))
		);
	});
	const port = await listen(server);
	const redirectUri = `http://127.0.0.1:${port}${CALLBACK_PATH}`;
	// One started while this one's listener was starting gives way to it.
	cancelChatGptSignIn();

	const url = new URL(endpoints().authorize);
	url.search = new URLSearchParams({
		client_id: clientId,
		...(again ? {} : { agent_name_hint: APP_NAME }),
		ext_agent_host_id: s.hostId,
		// A retained ID token picks the account without asking; after a sign-out there's none.
		...(again?.idToken ? { id_token_hint: again.idToken } : {}),
		...(again?.email ? { login_hint: again.email } : {}),
		// Asked again when the plan wasn't allowed last time.
		...(again && again.idToken && !again.scopes.includes(PLAN_SCOPE) ? { prompt: 'consent' } : {}),
		response_type: 'code',
		redirect_uri: redirectUri,
		scope: SCOPES.join(' '),
		resource: API_RESOURCE,
		state,
		nonce,
		code_challenge_method: 'S256',
		code_challenge: challenge
	}).toString();

	const timer = setTimeout(
		() => attempt.finish(new PlanError('Nobody finished signing in with ChatGPT in time.')),
		SIGN_IN_TIMEOUT_MS
	);
	timer.unref();

	/** Exchanges the code the browser brought back, checks it, and keeps the sign-in. */
	const exchange = async (back: URL): Promise<void> => {
		const params = back.searchParams;
		const error = params.get('error');
		if (error === 'access_denied') {
			throw new PlanError('The sign-in was turned down on ChatGPT’s page.');
		}
		if (error) {
			throw new PlanError(`ChatGPT's sign-in said: ${params.get('error_description') || error}.`);
		}
		const code = params.get('code');
		if (!code) throw new PlanError('That address has no sign-in code in it.');
		const issued = params.get('client_id');
		if (!again && (!issued || issued === 'dynamic_agent_client')) {
			throw new PlanError("ChatGPT didn't register nolune: start again.");
		}
		if (again && issued && issued !== again.clientId) {
			throw new PlanError('ChatGPT answered for another registration of nolune: start again.');
		}
		const exchangeClient = again?.clientId ?? issued!;
		const response = await postForm(endpoints().token, {
			grant_type: 'authorization_code',
			client_id: exchangeClient,
			code,
			code_verifier: verifier,
			redirect_uri: redirectUri,
			resource: API_RESOURCE
		}).catch((err: unknown) => {
			if (err instanceof AuthServiceError && err.code === 'invalid_grant') {
				throw new PlanError('That sign-in code was already used or ran out: start again.');
			}
			throw err instanceof AuthServiceError ? new PlanError(err.message, err.code) : err;
		});
		if (typeof response.id_token !== 'string') {
			throw new PlanError("ChatGPT's sign-in service sent no ID token.");
		}
		const identity = await verifyIdToken(response.id_token, { clientId: exchangeClient, nonce });
		if (again && again.subject !== identity.subject) {
			throw new PlanError(
				`That's another ChatGPT account than ${again.email ?? 'the one nolune knows'}: sign in with “Use another account” instead.`
			);
		}
		// A token response without `scope` granted what was asked for (RFC 6749, 5.1).
		const tokens = tokensOf(response, { idToken: null, refreshToken: null, scopes: SCOPES });
		const registration: Registration = {
			clientId: exchangeClient,
			subject: identity.subject,
			email: identity.email,
			plan: identity.plan,
			...tokens
		};
		// Under the lock, so a refresh in another process doesn't write over it.
		await withLock(async () => {
			const now = stored();
			now.registrations = [
				...now.registrations.filter((r) => r.clientId !== exchangeClient),
				registration
			];
			now.active = exchangeClient;
			writeStored(now);
		});
	};

	const attempt: Attempt = {
		signIn: { url: url.toString(), expiresAt: Date.now() + SIGN_IN_TIMEOUT_MS, done },
		complete: (back) => {
			if (finished) return Promise.reject(new PlanError('This sign-in has ended: start again.'));
			// An address from an older sign-in leaves this one waiting.
			if (back.searchParams.get('state') !== state) {
				return Promise.reject(
					new PlanError('That address is from another sign-in: use the one this sign-in ended on.')
				);
			}
			// The listener and a pasted address may both bring it back: the first one counts.
			completing ??= exchange(back).then(
				() => attempt.finish(null),
				(err: unknown) => {
					const error = err instanceof Error ? err : new Error(String(err));
					attempt.finish(error);
					throw error;
				}
			);
			return completing;
		},
		finish: (err, quiet = false) => {
			if (finished) return;
			finished = true;
			clearTimeout(timer);
			if (pending === attempt) pending = null;
			if (err && !quiet) lastError = err.message;
			// Its answers close their connections, so it ends once the last one is sent.
			server.close();
			if (err) settle.reject(err);
			else settle.resolve();
		}
	};
	pending = attempt;
	return attempt.signIn;
}

/**
 * Finishes the sign-in under way with the address its browser ended on: from another device, the
 * page on 127.0.0.1 it's sent back to doesn't load, but its address has what nolune needs.
 */
export async function finishChatGptSignIn(address: string): Promise<void> {
	const attempt = pending;
	if (!attempt) throw new PlanError('No sign-in with ChatGPT is under way: start one again.');
	let url: URL;
	try {
		url = new URL(address.trim());
	} catch {
		throw new PlanError("That isn't an address: copy the whole address the sign-in ended on.");
	}
	if (url.pathname !== CALLBACK_PATH || !url.searchParams.has('state')) {
		throw new PlanError(
			`That isn't where the sign-in ended: copy the address that starts with http://127.0.0.1 and has ${CALLBACK_PATH} in it.`
		);
	}
	await attempt.complete(url);
}

/** Stops waiting for the sign-in under way. */
export function cancelChatGptSignIn(): void {
	pending?.finish(new PlanError('The sign-in was cancelled.'), true);
}

/**
 * The sign-in under way in this process, if any, why the last one didn't finish, and the account
 * a new sign-in would sign in to again when none is signed in.
 */
export function chatGptSignInState(): {
	pending: { url: string; expiresAt: number } | null;
	signInError: string | null;
	previous: string | null;
} {
	const s = readStored();
	const signedIn = activeOf(s);
	const previous =
		signedIn && (signedIn.accessToken || signedIn.refreshToken) ? null : lastRegistration(s);
	return {
		pending: pending ? { url: pending.signIn.url, expiresAt: pending.signIn.expiresAt } : null,
		signInError: lastError,
		previous: previous ? (previous.email ?? previous.subject) : null
	};
}

/**
 * Signs out: asks OpenAI to end the sign-in, then forgets its tokens. The account's registration
 * stays, so signing in to it again reuses it. Returns false when OpenAI couldn't be told, in which
 * case it can be disconnected in ChatGPT's settings.
 */
export async function signOutChatGpt(): Promise<boolean> {
	cancelChatGptSignIn();
	lastError = null;
	return withLock(async () => {
		const s = readStored();
		const signedIn = activeOf(s);
		if (!s || !signedIn) return true;
		let revoked = true;
		if (signedIn.refreshToken) {
			revoked = false;
			for (let attempt = 0; attempt < 3 && !revoked; attempt++) {
				try {
					await postForm(endpoints().revoke, {
						token: signedIn.refreshToken,
						token_type_hint: 'refresh_token',
						client_id: signedIn.clientId
					});
					revoked = true;
				} catch (err) {
					// A 4xx won't change by trying again.
					if (err instanceof AuthServiceError && err.status && err.status < 500) break;
					await new Promise((resolve) => setTimeout(resolve, 500 * 2 ** attempt));
				}
			}
		}
		Object.assign(signedIn, {
			accessToken: null,
			refreshToken: null,
			idToken: null,
			expiresAt: null
		});
		s.active = null;
		writeStored(s);
		return revoked;
	});
}

// --- the access token ---

/** Codes of a refresh token that can't be used again: signing in again is the way back. */
const DEAD_REFRESH = new Set([
	'invalid_grant',
	'invalid_refresh_token',
	'token_expired',
	'refresh_token_expired',
	'refresh_token_invalidated',
	'refresh_token_reused'
]);

let refreshing: Promise<string> | null = null;

function notSignedIn(): PlanError {
	return new PlanError(
		`Nobody is signed in with ChatGPT. ${CHATGPT_SIGN_IN_HELP}`,
		'not_signed_in'
	);
}

function fresh(r: Registration, now = Date.now()): boolean {
	return !!r.accessToken && !!r.expiresAt && r.expiresAt - now > REFRESH_MARGIN_MS;
}

/** The signed-in account, or a PlanError saying nobody is, or that it can't use the plan. */
export function requireRegistration(): Registration {
	const r = activeRegistration();
	if (!r || (!r.refreshToken && !r.accessToken)) throw notSignedIn();
	if (!r.scopes.includes(PLAN_SCOPE)) {
		throw new PlanError(
			`${r.email ?? 'The account'} signed in without letting nolune use its ChatGPT plan. Sign in again and allow it: ${CHATGPT_SIGN_IN_HELP}`,
			'plan_not_allowed'
		);
	}
	return r;
}

/**
 * The access token for Responses API requests on the plan, refreshed first when it's about to
 * run out. Throws a PlanError when nobody is signed in or the sign-in has ended.
 */
export async function chatGptAccessToken(): Promise<string> {
	const r = requireRegistration();
	if (fresh(r)) return r.accessToken!;
	refreshing ??= refresh().finally(() => (refreshing = null));
	return refreshing;
}

async function refresh(): Promise<string> {
	return withLock(async () => {
		// Another process may have refreshed while this one waited.
		const s = readStored();
		const r = activeOf(s);
		if (!s || !r) throw notSignedIn();
		const now = Date.now();
		if (fresh(r, now)) return r.accessToken!;
		const usable = !!r.accessToken && !!r.expiresAt && r.expiresAt - CLOCK_SKEW_MS > now;
		// OpenAI says when refreshing is useful; until then the token in hand still works.
		if (usable && r.earliestRefreshAt && now < r.earliestRefreshAt) return r.accessToken!;
		if (!r.refreshToken) {
			throw new PlanError(`The ChatGPT sign-in has ended. ${CHATGPT_SIGN_IN_HELP}`, 'signed_out');
		}
		let response: TokenResponse;
		try {
			response = await postForm(endpoints().token, {
				grant_type: 'refresh_token',
				client_id: r.clientId,
				refresh_token: r.refreshToken,
				resource: API_RESOURCE
			});
		} catch (err) {
			if (err instanceof AuthServiceError && err.code && DEAD_REFRESH.has(err.code)) {
				Object.assign(r, { accessToken: null, refreshToken: null, expiresAt: null });
				writeStored(s);
				throw new PlanError(
					`The ChatGPT sign-in has ended (${err.code}): it was signed out, disconnected in ChatGPT's settings, or unused for 30 days. ${CHATGPT_SIGN_IN_HELP}`,
					err.code
				);
			}
			// Down for a moment: the token in hand still works until it runs out.
			if (usable) return r.accessToken!;
			if (err instanceof AuthServiceError) throw new PlanError(err.message, err.code);
			throw err;
		}
		Object.assign(r, tokensOf(response, r));
		writeStored(s);
		return r.accessToken!;
	});
}
