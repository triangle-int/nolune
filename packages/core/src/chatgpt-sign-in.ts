import { chmodSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { setTimeout as sleep } from 'node:timers/promises';
import { paths } from './paths.ts';
import { PlanError, describePlanAccount, type PlanAccount, type PlanStatus } from './plans.ts';

/*
 * Signing in with ChatGPT, for the ChatGPT plan (chatgpt-plan.ts): its chats run on a Plus, Pro or
 * Business plan through OpenAI's Codex backend, as the Codex CLI does, instead of on API credit.
 *
 * The sign-in is the Codex CLI's device code flow (`codex login --device-auth`): btw asks for a
 * one-time code, someone signed in to ChatGPT enters it at auth.openai.com/codex/device, and btw
 * gets the account's tokens. Nothing redirects to this computer, so it works through a tunnel
 * and from a phone. The client id is Codex's: it's the one ChatGPT's backend serves Codex to.
 *
 * The tokens are kept in chatgpt-auth.json next to config.json, readable only by this user, and
 * read on every request, like config.json. Before the access token runs out, btw trades the
 * refresh token for new ones. A refresh token works once, and the gateway and the CLI can both
 * refresh, so whoever finds the file changed under it uses what's there now.
 */

const CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann';

/** BTW_CHATGPT_ISSUER points it elsewhere, for tests. */
function issuer(): string {
	return (process.env.BTW_CHATGPT_ISSUER || 'https://auth.openai.com').replace(/\/+$/, '');
}

const REQUEST_TIMEOUT_MS = 30_000;
/** How long a code can be entered, as ChatGPT says in its own sign-in. */
const CODE_LIFETIME_MS = 15 * 60_000;
/** Refreshed this long before the access token runs out, so a turn doesn't start on one about to. */
const REFRESH_MARGIN_MS = 5 * 60_000;
/** How often Codex refreshes a token that doesn't say when it runs out. */
const REFRESH_EVERY_MS = 8 * 24 * 60 * 60_000;

export const CHATGPT_SIGN_IN_HELP =
	'An admin can sign in under Models & keys in btw, or with `btw chatgpt-plan setup`.';

function notSignedIn(): PlanError {
	return new PlanError(`Not signed in with ChatGPT. ${CHATGPT_SIGN_IN_HELP}`);
}

// --- what the tokens say ---

interface Claims {
	exp?: unknown;
	email?: unknown;
	chatgpt_account_id?: unknown;
	organizations?: { id?: unknown }[];
	'https://api.openai.com/auth'?: { chatgpt_account_id?: unknown; chatgpt_plan_type?: unknown };
	'https://api.openai.com/profile'?: { email?: unknown };
}

/** A JWT's claims, unchecked: the tokens came straight from ChatGPT, and btw only reads them. */
function claimsOf(jwt: string | undefined): Claims {
	const payload = jwt?.split('.')[1];
	if (!payload) return {};
	try {
		const claims = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8')) as unknown;
		return claims && typeof claims === 'object' ? (claims as Claims) : {};
	} catch {
		return {};
	}
}

function text(value: unknown): string | null {
	return typeof value === 'string' && value ? value : null;
}

/** The ChatGPT account (workspace) the tokens are for, which requests name in a header. */
function accountIdOf(...tokens: (string | undefined)[]): string | null {
	for (const token of tokens) {
		const claims = claimsOf(token);
		const id =
			text(claims['https://api.openai.com/auth']?.chatgpt_account_id) ??
			text(claims.chatgpt_account_id) ??
			text(claims.organizations?.[0]?.id);
		if (id) return id;
	}
	return null;
}

// --- the stored sign-in ---

interface StoredAuth {
	idToken: string;
	accessToken: string;
	refreshToken: string;
	accountId: string | null;
	/** When ChatGPT last issued tokens, as an ISO date. */
	refreshedAt: string;
}

function readAuth(): StoredAuth | null {
	let raw: string;
	try {
		raw = readFileSync(paths.chatgptAuth, 'utf8');
	} catch {
		return null;
	}
	try {
		const auth = JSON.parse(raw) as Partial<StoredAuth>;
		if (typeof auth.accessToken !== 'string' || typeof auth.refreshToken !== 'string') return null;
		return {
			idToken: typeof auth.idToken === 'string' ? auth.idToken : '',
			accessToken: auth.accessToken,
			refreshToken: auth.refreshToken,
			accountId: typeof auth.accountId === 'string' ? auth.accountId : null,
			refreshedAt: typeof auth.refreshedAt === 'string' ? auth.refreshedAt : ''
		};
	} catch {
		return null;
	}
}

/** Written whole and renamed into place, so another process never reads half of it. */
function writeAuth(auth: StoredAuth): void {
	mkdirSync(paths.home, { recursive: true });
	const temp = `${paths.chatgptAuth}.${process.pid}.tmp`;
	writeFileSync(temp, JSON.stringify(auth, null, '\t') + '\n', { mode: 0o600 });
	chmodSync(temp, 0o600);
	renameSync(temp, paths.chatgptAuth);
}

function fromTokens(
	tokens: { id_token?: unknown; access_token?: unknown; refresh_token?: unknown },
	previous?: StoredAuth
): StoredAuth | null {
	const accessToken = text(tokens.access_token);
	const refreshToken = text(tokens.refresh_token) ?? previous?.refreshToken;
	if (!accessToken || !refreshToken) return null;
	const idToken = text(tokens.id_token) ?? previous?.idToken ?? '';
	return {
		idToken,
		accessToken,
		refreshToken,
		accountId: accountIdOf(idToken, accessToken) ?? previous?.accountId ?? null,
		refreshedAt: new Date().toISOString()
	};
}

/** "ChatGPT Plus" for `plus`, as the Claude plan says "Claude Max". */
function planName(type: string | null): string | null {
	return type ? `ChatGPT ${type[0].toUpperCase()}${type.slice(1)}` : null;
}

/** Who btw is signed in as, or null. */
export function chatGptAccount(): PlanAccount | null {
	const auth = readAuth();
	if (!auth) return null;
	const id = claimsOf(auth.idToken);
	const access = claimsOf(auth.accessToken);
	return {
		email:
			text(id.email) ??
			text(id['https://api.openai.com/profile']?.email) ??
			text(access['https://api.openai.com/profile']?.email),
		plan: planName(
			text(id['https://api.openai.com/auth']?.chatgpt_plan_type) ??
				text(access['https://api.openai.com/auth']?.chatgpt_plan_type)
		)
	};
}

// --- a request's credentials ---

export interface ChatGptCredentials {
	accessToken: string;
	/** Sent as ChatGPT-Account-Id, when the tokens name one. */
	accountId: string | null;
}

function needsRefresh(auth: StoredAuth): boolean {
	const exp = claimsOf(auth.accessToken).exp;
	if (typeof exp === 'number') return exp * 1000 - REFRESH_MARGIN_MS <= Date.now();
	return !(Date.parse(auth.refreshedAt) + REFRESH_EVERY_MS > Date.now());
}

let refreshing: Promise<StoredAuth> | undefined;

/**
 * The signed-in account's token for a request, renewed first when it's about to run out, or
 * when it's `rejected`: the token the backend just turned down.
 */
export async function chatGptCredentials(rejected?: string): Promise<ChatGptCredentials> {
	let auth = readAuth();
	if (!auth) throw notSignedIn();
	const wasRejected = rejected !== undefined && auth.accessToken === rejected;
	if (needsRefresh(auth) || wasRejected) {
		const stale = auth;
		try {
			auth = await (refreshing ??= refresh(stale).finally(() => (refreshing = undefined)));
		} catch (err) {
			// Renewed early, so the token may still work for a few minutes: better than no answer.
			if (wasRejected || !stillValid(stale)) throw err;
		}
	}
	return { accessToken: auth.accessToken, accountId: auth.accountId };
}

function stillValid(auth: StoredAuth): boolean {
	const exp = claimsOf(auth.accessToken).exp;
	return typeof exp === 'number' && exp * 1000 > Date.now();
}

/** fetch says "fetch failed"; the reason is in its cause (ECONNREFUSED, ENOTFOUND...). */
function networkError(err: unknown): string {
	if (!(err instanceof Error)) return String(err);
	if (err.name === 'TimeoutError') return 'no answer';
	const cause = err.cause as { code?: unknown; message?: unknown } | undefined;
	return text(cause?.code) ?? text(cause?.message) ?? err.message;
}

/** A JSON object answer, or an empty one when it isn't one. */
async function jsonOf(res: Response): Promise<Record<string, unknown>> {
	try {
		const body = (await res.json()) as unknown;
		return body && typeof body === 'object' ? (body as Record<string, unknown>) : {};
	} catch {
		return {};
	}
}

/** The error code and message of an OAuth failure, in either of the shapes it comes in. */
async function oauthError(res: Response): Promise<{ code: string; message: string }> {
	const body = await jsonOf(res);
	if (body.error && typeof body.error === 'object') {
		const error = body.error as Record<string, unknown>;
		return { code: text(error.code) ?? '', message: text(error.message) ?? '' };
	}
	return {
		code: text(body.error) ?? text(body.code) ?? '',
		message: text(body.error_description) ?? text(body.message) ?? ''
	};
}

/** Refresh-token failures that no retry fixes, as Codex tells them apart. */
const ENDED = ['refresh_token_expired', 'refresh_token_reused', 'refresh_token_invalidated'];

async function refresh(auth: StoredAuth): Promise<StoredAuth> {
	let res: Response;
	try {
		res = await fetch(`${issuer()}/oauth/token`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				client_id: CLIENT_ID,
				grant_type: 'refresh_token',
				refresh_token: auth.refreshToken
			}),
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
		});
	} catch (err) {
		throw new PlanError(
			`Couldn't reach ChatGPT to renew its sign-in (${networkError(err)}).`,
			null,
			{ cause: err }
		);
	}
	const tokens = res.ok ? await jsonOf(res) : null;
	// The file may have changed meanwhile: signed out, signed in again, or renewed by another btw
	// process, which is also why ChatGPT would refuse this refresh token. What's there now wins.
	const current = readAuth();
	if (!current) throw notSignedIn();
	if (current.refreshToken !== auth.refreshToken) return current;

	if (!tokens) {
		const { code, message } = await oauthError(res);
		if (
			res.status === 401 ||
			ENDED.includes(code.toLowerCase()) ||
			(res.status === 400 && code === 'invalid_grant')
		) {
			throw new PlanError(
				`The ChatGPT sign-in has expired or was signed out. ${CHATGPT_SIGN_IN_HELP}`
			);
		}
		throw new PlanError(
			`ChatGPT couldn't renew its sign-in (${res.status}${message ? `: ${message}` : ''}). Try again in a moment.`
		);
	}
	const next = fromTokens(tokens, auth);
	if (!next) throw new PlanError('ChatGPT renewed its sign-in without a token.');
	writeAuth(next);
	return next;
}

// --- signing in and out ---

export interface ChatGptSignIn {
	/** Where to enter the code, signed in to ChatGPT. */
	verificationUrl: string;
	userCode: string;
	/** When the code stops working, in ms since the epoch. */
	expiresAt: number;
	/** Resolves once the code was entered and the tokens saved. Rejects when it expires or fails. */
	done: Promise<PlanAccount>;
}

/** The sign-in under way in this process, and why the last one didn't finish. */
let pending: { signIn: ChatGptSignIn | null; abort: AbortController } | null = null;
let lastError: string | null = null;

async function postJson(url: string, body: object, signal: AbortSignal): Promise<Response> {
	return fetch(url, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(body),
		signal: AbortSignal.any([signal, AbortSignal.timeout(REQUEST_TIMEOUT_MS)])
	});
}

/**
 * Starts signing in: asks ChatGPT for a one-time code and waits, in the background, for it to be
 * entered. Replaces a sign-in already under way. The tokens are saved when it's done, so the
 * next request uses the new account.
 */
export async function startChatGptSignIn(): Promise<ChatGptSignIn> {
	cancelChatGptSignIn();
	lastError = null;
	const attempt = { signIn: null as ChatGptSignIn | null, abort: new AbortController() };
	pending = attempt;
	const { signal } = attempt.abort;

	let res: Response;
	try {
		res = await postJson(
			`${issuer()}/api/accounts/deviceauth/usercode`,
			{ client_id: CLIENT_ID },
			signal
		);
	} catch (err) {
		if (pending === attempt) pending = null;
		if (signal.aborted) throw new PlanError('The sign-in was cancelled.');
		throw new PlanError(`Couldn't reach ChatGPT (${networkError(err)}).`, null, { cause: err });
	}
	const body = res.ok ? await jsonOf(res) : {};
	const deviceAuthId = text(body.device_auth_id);
	const userCode = text(body.user_code) ?? text(body.usercode);
	if (pending !== attempt) throw new PlanError('The sign-in was cancelled.');
	if (!deviceAuthId || !userCode) {
		pending = null;
		throw new PlanError(
			res.ok
				? "ChatGPT's answer had no code in it."
				: `ChatGPT couldn't start a sign-in (${res.status}). Try again in a moment.`
		);
	}

	const intervalMs = Math.max(1, Number(body.interval) || 5) * 1000;
	const expiresAt = Date.now() + CODE_LIFETIME_MS;
	const done = waitForCode({ deviceAuthId, userCode, intervalMs, expiresAt, signal });
	const signIn: ChatGptSignIn = {
		verificationUrl: `${issuer()}/codex/device`,
		userCode,
		expiresAt,
		done
	};
	attempt.signIn = signIn;
	// Handled here, so a sign-in nobody waits on (the web page's) never rejects unhandled.
	done.then(
		() => {
			if (pending === attempt) pending = null;
		},
		(err: unknown) => {
			if (pending !== attempt) return;
			pending = null;
			lastError = err instanceof Error ? err.message : String(err);
		}
	);
	return signIn;
}

/** Polls until the code is entered, then trades what ChatGPT hands back for tokens. */
async function waitForCode(opts: {
	deviceAuthId: string;
	userCode: string;
	intervalMs: number;
	expiresAt: number;
	signal: AbortSignal;
}): Promise<PlanAccount> {
	const { signal } = opts;
	for (;;) {
		try {
			await sleep(opts.intervalMs, undefined, { signal });
		} catch {
			throw new PlanError('The sign-in was cancelled.');
		}
		if (Date.now() >= opts.expiresAt) {
			throw new PlanError('The code expired before it was entered. Start the sign-in again.');
		}
		let res: Response;
		try {
			res = await postJson(
				`${issuer()}/api/accounts/deviceauth/token`,
				{ device_auth_id: opts.deviceAuthId, user_code: opts.userCode },
				signal
			);
		} catch {
			if (signal.aborted) throw new PlanError('The sign-in was cancelled.');
			// The network may come back before the code expires.
			continue;
		}
		// Not entered yet.
		if (res.status === 403 || res.status === 404) continue;
		if (!res.ok) {
			throw new PlanError(`ChatGPT turned the sign-in down (${res.status}).`);
		}
		const grant = await jsonOf(res);
		const code = text(grant.authorization_code);
		const verifier = text(grant.code_verifier);
		if (!code || !verifier) throw new PlanError("ChatGPT's answer had no sign-in in it.");
		return exchangeCode(code, verifier, signal);
	}
}

async function exchangeCode(
	code: string,
	verifier: string,
	signal: AbortSignal
): Promise<PlanAccount> {
	let res: Response;
	try {
		res = await fetch(`${issuer()}/oauth/token`, {
			method: 'POST',
			headers: { 'content-type': 'application/x-www-form-urlencoded' },
			body: new URLSearchParams({
				grant_type: 'authorization_code',
				client_id: CLIENT_ID,
				code,
				redirect_uri: `${issuer()}/deviceauth/callback`,
				code_verifier: verifier
			}),
			signal: AbortSignal.any([signal, AbortSignal.timeout(REQUEST_TIMEOUT_MS)])
		});
	} catch (err) {
		if (signal.aborted) throw new PlanError('The sign-in was cancelled.');
		throw new PlanError(
			`Couldn't reach ChatGPT to finish signing in (${networkError(err)}).`,
			null,
			{ cause: err }
		);
	}
	if (!res.ok) {
		const { message } = await oauthError(res);
		throw new PlanError(
			`ChatGPT couldn't finish the sign-in (${res.status}${message ? `: ${message}` : ''}).`
		);
	}
	const auth = fromTokens(await jsonOf(res));
	if (!auth) throw new PlanError('ChatGPT finished the sign-in without a token.');
	if (signal.aborted) throw new PlanError('The sign-in was cancelled.');
	writeAuth(auth);
	return chatGptAccount() ?? { email: null, plan: null };
}

/** Stops waiting for the code of a sign-in under way. The code then goes unused. */
export function cancelChatGptSignIn(): void {
	pending?.abort.abort();
	pending = null;
}

/**
 * Forgets the sign-in. ChatGPT is asked to revoke it too, as Codex does on logout, but the
 * tokens are deleted here even when that fails.
 */
export async function signOutChatGpt(): Promise<void> {
	cancelChatGptSignIn();
	lastError = null;
	const auth = readAuth();
	rmSync(paths.chatgptAuth, { force: true });
	if (!auth) return;
	try {
		await fetch(`${issuer()}/oauth/revoke`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				token: auth.refreshToken,
				token_type_hint: 'refresh_token',
				client_id: CLIENT_ID
			}),
			signal: AbortSignal.timeout(10_000)
		});
	} catch {
		// Best effort.
	}
}

export interface ChatGptPlanStatus extends PlanStatus {
	account: PlanAccount | null;
	/** A sign-in in this process that's waiting for its code. */
	pending: Omit<ChatGptSignIn, 'done'> | null;
	/** Why the last sign-in in this process didn't finish. */
	signInError: string | null;
}

/**
 * For Models & keys, `btw chatgpt-plan status` and `btw config`. Never the tokens. Unlike the
 * Claude plan's, it asks nobody: the sign-in is btw's own.
 */
export function chatGptPlanStatus(): ChatGptPlanStatus {
	const signIn = pending?.signIn;
	const account = chatGptAccount();
	return {
		account,
		signedIn: account && describePlanAccount(account),
		problem: account ? null : notSignedIn().message,
		signInError: lastError,
		pending: signIn
			? {
					verificationUrl: signIn.verificationUrl,
					userCode: signIn.userCode,
					expiresAt: signIn.expiresAt
				}
			: null
	};
}
