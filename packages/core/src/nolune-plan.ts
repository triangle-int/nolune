import { chmodSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import type { ChatApi } from './openrouter.ts';
import type { ImageApi } from './openrouter-images.ts';
import { paths } from './paths.ts';
import { PlanError, describePlanAccount, type PlanAccount, type PlanStatus } from './plans.ts';

/*
 * The nolune plan: a subscription to nolune itself, whose API (packages/api, at api.nolune.dev)
 * passes chats, pictures and embeddings on to OpenRouter (DESIGN.md, The nolune plan). Linking this gateway
 * to it is a device code (OAuth's device authorization grant): nolune asks the API for one, shows
 * its code and link, and asks every few seconds until someone signed in on the link page approves
 * it. The API then gives a token, kept in ~/.nolune/nolune-plan.json and sent as a bearer token on
 * every request. Chats run in nolune's own loop through openrouter.ts, whose Chat Completions the
 * API speaks (NOLUNE_PLAN below).
 *
 * What the API refuses (a limit reached, the plan's credits spent, a link that ended) is said as a
 * PlanError, in words for the people in the chat, with when the limit starts again.
 */

/** The client id a gateway asks for a device code with (the API's GATEWAY_CLIENT). */
const CLIENT_ID = 'nolune';
const DEFAULT_API_URL = 'https://api.nolune.dev';
const REQUEST_TIMEOUT_MS = 30_000;

/** Where nolune's API is. NOLUNE_PLAN_API_URL points it at another: a local one, or a stand-in. */
export function nolunePlanApiUrl(): string {
	return (process.env.NOLUNE_PLAN_API_URL || DEFAULT_API_URL).replace(/\/+$/, '');
}

/** Whether nolune's API is open to everyone. Until it is, the web UI offers the plan only when
 * NOLUNE_PLAN_API_URL points nolune at an API (a local one, in development); the CLI always has it. */
const OPEN = false;

/** Whether the web UI offers the plan: Models & keys, the welcome, Add a model. */
export function nolunePlanOffered(): boolean {
	return OPEN || !!process.env.NOLUNE_PLAN_API_URL;
}

/** The account page on nolune's API: where people sign in, subscribe, buy credits and manage it. */
export function nolunePlanAccountUrl(): string {
	return `${nolunePlanApiUrl()}/`;
}

export const NOLUNE_PLAN_HELP =
	'Link nolune to the plan again: `nolune nolune-plan setup`, or the nolune plan under Models & keys.';

// --- what's kept ---

interface Stored {
	/** The API the token is for: it's never sent to another. */
	api: string;
	token: string;
	email: string | null;
	linkedAt: string;
}

function readStored(): Stored | null {
	let stored: Partial<Stored>;
	try {
		stored = JSON.parse(readFileSync(paths.nolunePlan, 'utf8'));
	} catch {
		return null;
	}
	if (typeof stored.token !== 'string' || stored.api !== nolunePlanApiUrl()) return null;
	return {
		api: stored.api,
		token: stored.token,
		email: typeof stored.email === 'string' ? stored.email : null,
		linkedAt: String(stored.linkedAt ?? '')
	};
}

/** Readable by this user only, written whole and renamed into place, as chatgpt.json is. */
function writeStored(stored: Stored): void {
	mkdirSync(dirname(paths.nolunePlan), { recursive: true });
	const temp = `${paths.nolunePlan}.${process.pid}.tmp`;
	writeFileSync(temp, JSON.stringify(stored, null, '\t') + '\n', { mode: 0o600 });
	chmodSync(temp, 0o600);
	renameSync(temp, paths.nolunePlan);
}

function forget(): void {
	rmSync(paths.nolunePlan, { force: true });
	if (latest || noPlanAt !== null) setUsage(null);
}

function notLinked(): PlanError {
	return new PlanError(`nolune isn't linked to a nolune plan. ${NOLUNE_PLAN_HELP}`, 'not_linked');
}

/** The token requests go with; a PlanError when nolune isn't linked. */
export function nolunePlanToken(): string {
	const stored = readStored();
	if (!stored) throw notLinked();
	return stored.token;
}

/** Throws a PlanError unless nolune is linked to the plan; asks nothing. */
export function requireNolunePlan(): void {
	nolunePlanToken();
}

// --- what goes wrong, in words ---

/** When a limit starts again, as the people in the chat read it: "18:40", or "Friday 18:40". */
function whenAgain(iso: unknown, now = new Date()): string | null {
	if (typeof iso !== 'string') return null;
	const at = new Date(iso);
	if (Number.isNaN(at.getTime())) return null;
	const time = at.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
	if (at.toDateString() === now.toDateString()) return `at ${time}`;
	const day = at.toLocaleDateString('en-GB', { weekday: 'long' });
	return at.getTime() - now.getTime() < 6 * 24 * 60 * 60 * 1000
		? `on ${day} at ${time}`
		: `on ${at.toLocaleDateString('en-GB', { day: 'numeric', month: 'long' })}`;
}

/**
 * What nolune's API said went wrong, from its status and its error (OpenAI's shape:
 * `{ error: { code, message, resets_at } }`), as a PlanError; null when it's nothing of the plan's.
 */
export function planRefusal(status: number, error: unknown): PlanError | null {
	const found = (error ?? {}) as { code?: unknown; message?: unknown; resets_at?: unknown };
	const code = typeof found.code === 'string' ? found.code : null;
	const again = whenAgain(found.resets_at);
	switch (code) {
		case 'five_hour_limit':
			return new PlanError(
				`The nolune plan's 5-hour limit is reached. Chats start again ${again ?? 'within 5 hours'}.`,
				code
			);
		case 'weekly_limit':
			return new PlanError(
				`The nolune plan's weekly limit is reached. Chats start again ${again ?? 'next week'}.`,
				code
			);
		case 'background_share':
			return new PlanError(
				`Background work has used its share of the nolune plan's limits, to keep the rest for people's chats. It starts again ${again ?? 'when the limit does'}.`,
				code
			);
		case 'credits_spent':
			return new PlanError(
				`The nolune plan's credits are spent until the next payment${again ? `, ${again.replace(/^on /, '')}` : ''}.`,
				code
			);
		case 'no_plan':
			return new PlanError(
				`The account nolune is linked to has no nolune plan. Subscribe at ${nolunePlanApiUrl()}, or link nolune to another account.`,
				code
			);
		case 'not_signed_in':
			return new PlanError(`nolune's link to the nolune plan has ended. ${NOLUNE_PLAN_HELP}`, code);
		case 'upstream_unavailable':
		case 'upstream_unreachable':
			return new PlanError(
				"nolune's API can't reach the models just now. Try again in a little while.",
				code
			);
		case 'too_many_requests':
			return new PlanError(
				'Too many requests on the nolune plan at once. Try again in a moment.',
				code
			);
		case 'model_not_offered':
			return new PlanError(
				typeof found.message === 'string'
					? found.message
					: "The nolune plan doesn't offer this model.",
				code
			);
	}
	if (status === 401) {
		return new PlanError(
			`nolune's link to the nolune plan has ended. ${NOLUNE_PLAN_HELP}`,
			'not_signed_in'
		);
	}
	return null;
}

/**
 * What a call to nolune's API threw, as nolune says it: OpenAI's SDK's errors (the plan's chats
 * use it) and fetch's, as PlanErrors. A stop stays what it is, so the runner sees one.
 */
export function planErrorOf(err: unknown): unknown {
	if (err instanceof PlanError) return err;
	const e = err as { name?: unknown; status?: unknown; error?: unknown; cause?: unknown } | null;
	if (!e || typeof e !== 'object') return err;
	if (e.name === 'APIUserAbortError' || e.name === 'AbortError') return err;
	if (typeof e.status === 'number') {
		if (e.status === 401) forget();
		const refused = planRefusal(e.status, e.error);
		if (refused) return refused;
		const message = (e.error as { message?: unknown } | undefined)?.message;
		return new PlanError(
			`nolune's API answered ${e.status}${typeof message === 'string' ? `: ${message}` : '.'}`,
			null,
			{ cause: err }
		);
	}
	if (e.name === 'APIConnectionTimeoutError' || e.name === 'TimeoutError') {
		return new PlanError("nolune's API didn't answer in time.", null, { cause: err });
	}
	if (e.name === 'APIConnectionError' || err instanceof TypeError) {
		const cause = e.cause as { code?: string; cause?: { code?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code;
		return new PlanError(`Couldn't reach nolune's API${why ? ` (${why})` : ''}.`, null, {
			cause: err
		});
	}
	return err instanceof Error ? new PlanError(err.message, null, { cause: err }) : err;
}

async function asPlan<T>(call: () => Promise<T>): Promise<T> {
	try {
		return await call();
	} catch (err) {
		throw planErrorOf(err);
	} finally {
		// What it cost is charged once it's done: ask where the limits stand now.
		usageSoon();
	}
}

// --- where the limits stand ---

/** The usage the API last said, and when. */
let latest: { usage: NolunePlanUsage; at: number } | null = null;
/** When the API last said the linked account has no plan: null while it has one, or isn't known. */
let noPlanAt: number | null = null;
const usageListeners = new Set<() => void>();
let asking: Promise<void> | null = null;
let soon: ReturnType<typeof setTimeout> | null = null;
/** After this long, a usage that's read is asked for again. */
const USAGE_STALE_MS = 10 * 60 * 1000;
/** And an account with no plan, sooner: someone may be subscribing on the account page. */
const NO_PLAN_STALE_MS = 60 * 1000;
/**
 * How long after a request ends the usage is asked for: the API charges a stream once it has
 * ended, and requests that end together are asked about once.
 */
const USAGE_AFTER_MS = 1500;

function setUsage(usage: NolunePlanUsage | null): void {
	latest = usage ? { usage, at: Date.now() } : null;
	noPlanAt = null;
	for (const listener of usageListeners) listener();
}

/**
 * The API says the linked account has no plan. Listeners hear it when it's news, not on every ask:
 * pages ask again when they hear, so telling them each time would have them ask on and on.
 */
function setNoPlan(): void {
	const news = noPlanAt === null || latest !== null;
	latest = null;
	noPlanAt = Date.now();
	if (news) for (const listener of usageListeners) listener();
}

/** Asks the API where the limits stand (`/v1/usage`), once at a time; nothing is charged. */
function askUsage(): Promise<void> {
	asking ??= nolunePlanStatus({ check: true })
		.then(
			() => {},
			() => {}
		)
		.finally(() => (asking = null));
	return asking;
}

function usageSoon(): void {
	if (soon) clearTimeout(soon);
	soon = setTimeout(() => {
		soon = null;
		void askUsage();
	}, USAGE_AFTER_MS);
	// A command run in a terminal doesn't wait for it.
	soon.unref?.();
}

/** Calls `listener` whenever where the limits stand changes. Returns the way to stop. */
export function onNolunePlanUsage(listener: () => void): () => void {
	usageListeners.add(listener);
	return () => {
		usageListeners.delete(listener);
	};
}

/**
 * Where the plan's limits stand, as the API last said, and when: null when nolune isn't linked
 * or it isn't known yet. When it isn't known, or is old, it's asked for, and listeners hear when
 * it comes.
 */
export function nolunePlanUsage(): { usage: NolunePlanUsage; at: number } | null {
	if (!readStored()) {
		if (latest || noPlanAt !== null) setUsage(null);
		return null;
	}
	const now = Date.now();
	const known = latest
		? now - latest.at <= USAGE_STALE_MS
		: noPlanAt !== null && now - noPlanAt <= NO_PLAN_STALE_MS;
	if (!known) void askUsage();
	return latest;
}

/** The plan's chats: openrouter.ts's code, on nolune's API with the plan's token. */
export const NOLUNE_PLAN: ChatApi = {
	provider: 'nolune-plan',
	label: 'the nolune plan',
	baseURL: () => `${nolunePlanApiUrl()}/v1`,
	key: () => {
		// Throws when nolune isn't linked, before any request; the SDK asks again on each one.
		nolunePlanToken();
		return planToken;
	},
	wrap: asPlan,
	saysUse: true
};

async function planToken(): Promise<string> {
	return nolunePlanToken();
}

/**
 * The plan's pictures: openrouter-images.ts's requests, on nolune's API with the plan's token.
 * `nolune generate image` is a command the agent runs inside a turn, so it goes on with that turn,
 * and a hidden chat's (NOLUNE_USE, which the runner sets) is background work.
 */
export const NOLUNE_PLAN_IMAGES: ImageApi = {
	provider: 'nolune-plan',
	label: 'the nolune plan',
	baseURL: () => `${nolunePlanApiUrl()}/v1`,
	key: nolunePlanToken,
	headers: () => ({
		'X-Nolune-Use': process.env.NOLUNE_USE === 'background' ? 'background' : 'person',
		...(process.env.NOLUNE_CONVERSATION_ID ? { 'X-Nolune-Turn': 'continue' } : {})
	}),
	failure: (status, error) => {
		if (status === 401) forget();
		if (status === 413) {
			return new PlanError(
				'The pictures to start from are too large to send to the nolune plan.',
				null
			);
		}
		const message = typeof error?.message === 'string' ? `: ${error.message}` : '.';
		return (
			planRefusal(status, error) ?? new PlanError(`nolune's API answered ${status}${message}`, null)
		);
	},
	after: usageSoon
};

// --- linking ---

async function call(
	path: string,
	init: { method?: string; token?: string; json?: unknown; signal?: AbortSignal } = {}
): Promise<{ status: number; body: Record<string, unknown> | null }> {
	const response = await fetch(`${nolunePlanApiUrl()}${path}`, {
		method: init.method ?? (init.json === undefined ? 'GET' : 'POST'),
		headers: {
			...(init.json === undefined ? {} : { 'content-type': 'application/json' }),
			...(init.token ? { authorization: `Bearer ${init.token}` } : {})
		},
		body: init.json === undefined ? undefined : JSON.stringify(init.json),
		signal: init.signal ?? AbortSignal.timeout(REQUEST_TIMEOUT_MS)
	});
	const body = (await response.json().catch(() => null)) as Record<string, unknown> | null;
	return { status: response.status, body };
}

export interface NolunePlanSignIn {
	/** The code to check on the link page: `CW53-R2HT`. */
	code: string;
	/** The link page, with the code in it. */
	url: string;
	/** When the code runs out, in ms since the epoch. */
	expiresAt: number;
	/** Resolves once nolune is linked; rejects when it isn't. */
	done: Promise<void>;
}

/** The link under way in this process, and why the last one didn't finish. */
let pending: { signIn: NolunePlanSignIn; controller: AbortController } | null = null;
let lastError: string | null = null;

/** Four and four, as people read a code aloud: CW53R2HT is CW53-R2HT. */
function readable(code: string): string {
	return code.length === 8 ? `${code.slice(0, 4)}-${code.slice(4)}` : code;
}

function wait(ms: number, signal: AbortSignal): Promise<void> {
	return new Promise((resolve, reject) => {
		if (signal.aborted) return reject(new PlanError('The link was cancelled.'));
		const timer = setTimeout(resolve, ms);
		signal.addEventListener(
			'abort',
			() => {
				clearTimeout(timer);
				reject(new PlanError('The link was cancelled.'));
			},
			{ once: true }
		);
	});
}

const RAN_OUT = 'The code ran out before it was linked. Start again for a new one.';

/** Asks every `interval` seconds until the code is approved, turned down, or runs out. */
async function poll(
	deviceCode: string,
	interval: number,
	expiresAt: number,
	signal: AbortSignal
): Promise<void> {
	let every = interval * 1000;
	while (Date.now() < expiresAt) {
		await wait(every, signal);
		let answer: Awaited<ReturnType<typeof call>>;
		try {
			answer = await call('/api/auth/device/token', {
				json: {
					grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
					device_code: deviceCode,
					client_id: CLIENT_ID
				}
			});
		} catch {
			// The API didn't answer this time; the code may still be linked before it runs out.
			continue;
		}
		const token = answer.body?.access_token;
		if (answer.status === 200 && typeof token === 'string') {
			const usage = await call('/v1/usage', { token }).catch(() => null);
			const email = usage?.body?.email;
			writeStored({
				api: nolunePlanApiUrl(),
				token,
				email: typeof email === 'string' ? email : null,
				linkedAt: new Date().toISOString()
			});
			return;
		}
		switch (answer.body?.error) {
			case 'authorization_pending':
				continue;
			case 'slow_down':
				every += 5000;
				continue;
			case 'access_denied':
				throw new PlanError('Not linked: it was turned down on the link page.');
			case 'expired_token':
				throw new PlanError(RAN_OUT);
		}
		const why = answer.body?.error_description ?? answer.body?.error ?? answer.status;
		throw new PlanError(`nolune's API didn't link nolune (${String(why)}).`);
	}
	throw new PlanError(RAN_OUT);
}

/**
 * Starts linking nolune to the plan, or returns the link under way: the code and page to show,
 * and `done`, which settles once it's linked (replacing any link before) or not.
 */
export async function startNolunePlanSignIn(): Promise<NolunePlanSignIn> {
	if (pending) return pending.signIn;
	lastError = null;
	let asked: Awaited<ReturnType<typeof call>>;
	try {
		asked = await call('/api/auth/device/code', { json: { client_id: CLIENT_ID } });
	} catch (err) {
		throw planErrorOf(err);
	}
	const body = asked.body ?? {};
	const { device_code, user_code, verification_uri, verification_uri_complete } = body;
	if (asked.status !== 200 || typeof device_code !== 'string' || typeof user_code !== 'string') {
		throw new PlanError(`nolune's API didn't give a code to link with (${asked.status}).`);
	}
	const expiresAt = Date.now() + (Number(body.expires_in) || 900) * 1000;
	const interval = typeof body.interval === 'number' ? body.interval : 5;
	const controller = new AbortController();
	const signIn: NolunePlanSignIn = {
		code: readable(user_code),
		url:
			typeof verification_uri_complete === 'string'
				? verification_uri_complete
				: `${String(verification_uri)}?user_code=${user_code}`,
		expiresAt,
		done: Promise.resolve()
	};
	// Whoever awaits `done` sees the link over: no longer pending, and why it failed.
	const over = () => {
		if (pending?.signIn === signIn) pending = null;
	};
	signIn.done = poll(device_code, interval, expiresAt, controller.signal).then(
		() => {
			over();
			lastError = null;
		},
		(err: unknown) => {
			over();
			if (!controller.signal.aborted) lastError = (err as Error).message;
			throw err;
		}
	);
	// A page that started it asks for the state instead of waiting on `done`.
	signIn.done.catch(() => {});
	pending = { signIn, controller };
	return signIn;
}

/** Stops the link under way, if any, quietly. */
export function cancelNolunePlanSignIn(): void {
	pending?.controller.abort();
}

/** The link under way in this process, if any, and why the last one didn't finish. */
export function nolunePlanSignInState(): {
	pending: { code: string; url: string; expiresAt: number } | null;
	signInError: string | null;
} {
	const signIn = pending?.signIn;
	return {
		pending: signIn ? { code: signIn.code, url: signIn.url, expiresAt: signIn.expiresAt } : null,
		signInError: lastError
	};
}

/**
 * Ends the link: asks the API to end the token's session, then forgets it. Returns false when the
 * API couldn't be told; the token is forgotten either way, and ends by itself unused.
 */
export async function signOutNolunePlan(): Promise<boolean> {
	cancelNolunePlanSignIn();
	lastError = null;
	const stored = readStored();
	if (!stored) return true;
	let told = false;
	try {
		const answer = await call('/api/auth/sign-out', {
			token: stored.token,
			json: {},
			signal: AbortSignal.timeout(10_000)
		});
		told = answer.status === 200;
	} catch {
		// told stays false
	}
	forget();
	return told;
}

// --- the link, as Models & keys and `nolune nolune-plan status` show it ---

/** How much of each limit is used, as nolune's API says it (`x-nolune-usage`): millionths of a dollar. */
export interface NolunePlanUsage {
	window: { spent: number; limit: number; resetsAt: number | null };
	week: { spent: number; limit: number; resetsAt: number };
	credits: { plan: number; extra: number; renewsAt: number | null };
}

export interface NolunePlanStatus extends PlanStatus {
	account: PlanAccount | null;
	/** Where the limits stand, when the API was asked. */
	usage: NolunePlanUsage | null;
	/**
	 * The account nolune is linked to has no plan, as the API last said: it's subscribed to on the
	 * account page (`nolunePlanAccountUrl`).
	 */
	noPlan: boolean;
}

/**
 * Who nolune is linked as and what stops chats on the plan, from what nolune keeps. With `check`,
 * it also asks the API, which says where the limits stand: nothing is charged. Nothing needs
 * installing, so `installed` is always true.
 */
export async function nolunePlanStatus(opts: { check?: boolean } = {}): Promise<NolunePlanStatus> {
	const stored = readStored();
	const account: PlanAccount | null = stored ? { email: stored.email, plan: null } : null;
	const status = {
		path: null,
		installed: true,
		account,
		signedIn: account && describePlanAccount(account),
		usage: null,
		noPlan: !!stored && noPlanAt !== null
	};
	if (!stored) return { ...status, problem: notLinked().message };
	if (!opts.check) return { ...status, problem: null };
	let answer: Awaited<ReturnType<typeof call>>;
	try {
		answer = await call('/v1/usage', { token: stored.token });
	} catch (err) {
		return { ...status, problem: (planErrorOf(err) as Error).message };
	}
	if (answer.status === 401) {
		forget();
		return {
			...status,
			account: null,
			signedIn: null,
			noPlan: false,
			problem: `nolune's link to the nolune plan has ended. ${NOLUNE_PLAN_HELP}`
		};
	}
	if (answer.status !== 200) {
		const refused = planRefusal(answer.status, answer.body?.error);
		return { ...status, problem: refused?.message ?? `nolune's API answered ${answer.status}.` };
	}
	const usage = (answer.body?.usage ?? null) as NolunePlanUsage | null;
	if (!usage) {
		setNoPlan();
		return { ...status, noPlan: true, problem: planRefusal(402, { code: 'no_plan' })!.message };
	}
	setUsage(usage);
	return { ...status, usage, noPlan: false, problem: null };
}

/** Throws a PlanError unless nolune is linked to a plan the API takes. */
export async function checkNolunePlan(): Promise<NolunePlanStatus> {
	const status = await nolunePlanStatus({ check: true });
	if (status.problem) throw new PlanError(status.problem);
	return status;
}
