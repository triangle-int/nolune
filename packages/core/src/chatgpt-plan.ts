import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import {
	CHATGPT_SIGN_IN_HELP,
	CHATGPT_USAGE_URL,
	activeRegistration,
	chatGptAccessToken,
	planAccountOf,
	requireRegistration
} from './chatgpt-sign-in.ts';
import type { Message } from './format.ts';
import { EFFORTS, type Effort, type ModelChoice, type StreamEvent } from './models.ts';
import * as openai from './openai-chat.ts';
import { PlanError, describePlanAccount, type PlanAccount, type PlanStatus } from './plans.ts';

/*
 * Chats on the ChatGPT plan: the Plus or Pro plan of someone signed in with ChatGPT
 * (chatgpt-sign-in.ts). They go through OpenAI's Responses API like chats on an OpenAI key, in
 * nolune's own loop (openai-chat.ts does the requests), with the plan's access token instead of a
 * key, and with what the plan asks of a request:
 *
 * - streamed and stateless (`store: false`, the whole transcript every time), as nolune's are;
 * - no `max_output_tokens`, `temperature`, `metadata` and the like, which nolune doesn't send;
 * - function tools grouped in a namespace (nolune's);
 * - no Files API: pictures and PDFs go inline.
 *
 * What goes wrong with the plan (its limit, an account that can't use it, a sign-in that ended)
 * is said as a PlanError, in words for the people in the chat.
 */

const DEFAULT_API_URL = 'https://api.openai.com/v1';
/** How long the plan's models are remembered, for the efforts each takes. */
const CATALOG_TTL_MS = 5 * 60 * 1000;
const REQUEST_TIMEOUT_MS = 30_000;

/** Codex's latest release when this was written: the least the model catalog is asked as. */
const CODEX_VERSION = '0.160.0';
/** Codex's latest release, as npm says. */
const DEFAULT_CODEX_RELEASE_URL = 'https://registry.npmjs.org/@openai/codex/latest';
const CODEX_RELEASE_TTL_MS = 60 * 60 * 1000;
const CODEX_RELEASE_TIMEOUT_MS = 5_000;

/** Where the plan's requests go. NOLUNE_CHATGPT_API_URL points it at a stand-in, for tests. */
function apiUrl(): string {
	return (process.env.NOLUNE_CHATGPT_API_URL || DEFAULT_API_URL).replace(/\/+$/, '');
}

/** Where Codex's latest release is asked for. NOLUNE_CODEX_RELEASE_URL points it at a stand-in. */
function codexReleaseUrl(): string {
	return process.env.NOLUNE_CODEX_RELEASE_URL || DEFAULT_CODEX_RELEASE_URL;
}

let cached: { baseURL: string; client: OpenAI } | undefined;

/**
 * A client that asks for the access token on every request, so a refreshed one is used at once.
 * Never OPENAI_BASE_URL, an organization or a project from the environment: those are for the
 * OpenAI key, and the plan's token goes to OpenAI only.
 */
async function getClient(): Promise<OpenAI> {
	const baseURL = apiUrl();
	if (!cached || cached.baseURL !== baseURL) {
		const { OpenAI: Client } = await import('openai');
		cached = {
			baseURL,
			client: new Client({
				apiKey: chatGptAccessToken,
				baseURL,
				organization: null,
				project: null,
				adminAPIKey: null
			})
		};
	}
	return cached.client;
}

/** The plan's requests, through openai-chat.ts. */
export const CHATGPT_PLAN: openai.ResponsesApi = {
	provider: 'chatgpt-plan',
	client: getClient,
	// A registration's client id is its own and no secret.
	account: () => requireRegistration().clientId,
	modelName: (model) => model
};

// --- what goes wrong ---

/** Who's signed in, for messages about the account. */
function who(): string {
	return activeRegistration()?.email ?? 'This ChatGPT account';
}

/**
 * The plan's failures in words for the people in the chat (developers.openai.com/siwc, Errors and
 * recovery). Null for failures OpenAI's own words say well enough.
 */
function describeFailure(failure: {
	status: number | null;
	code: string | null;
	param: string | null;
	message: string;
}): string | null {
	const { status, code, param, message } = failure;
	switch (code) {
		case 'subscription_sharing_usage_limit_exceeded':
			return `The ChatGPT plan's usage limit is reached: the plan's own, or the one set for nolune in ChatGPT's settings. See it, and when it resets, at ${CHATGPT_USAGE_URL}.`;
		case 'subscription_sharing_user_not_eligible':
			return `${who()} can't use its ChatGPT plan in other apps: that takes ChatGPT Plus or Pro.`;
		case 'subscription_sharing_usage_unavailable':
		case 'subscription_sharing_user_unavailable':
			return "ChatGPT couldn't check the plan just now. Try again in a little while.";
		case 'subscription_sharing_unsupported_capability':
			return `The ChatGPT plan doesn't take ${param ? `\`${param}\`` : 'something in this request'}: ${message}`;
		case 'subscription_sharing_route_not_supported':
			return `The ChatGPT plan doesn't take this kind of request: ${message}`;
		case 'subscription_sharing_invalid_user':
			return `ChatGPT couldn't check who's signed in (${message}). ${CHATGPT_SIGN_IN_HELP}`;
		case 'chatpass_v2_scope_not_authorized':
		case 'chatpass_v2_invalid_authorization_context':
			return `ChatGPT didn't allow this for nolune's sign-in (${code}). ${CHATGPT_SIGN_IN_HELP}`;
	}
	if (status === 401) {
		return `ChatGPT didn't accept nolune's sign-in: it may have been disconnected in ChatGPT's settings. ${CHATGPT_SIGN_IN_HELP}`;
	}
	if (status === 403) {
		return `ChatGPT turned nolune down${code ? ` (${code})` : ''}: a policy or permission check, such as where this computer is, stopped the request.`;
	}
	if (status === 503 && !code) {
		return "ChatGPT's plan isn't taking requests from nolune just now. Try again in a little while.";
	}
	return null;
}

/** `err` as a PlanError when it's about the plan; aborts and other failures as they are. */
function planError(err: unknown): unknown {
	if (err instanceof PlanError || openai.isAbortError(err)) return err;
	// From the client's token callback, which the SDK wraps.
	const cause = (err as { cause?: unknown } | null)?.cause;
	if (cause instanceof PlanError) return cause;
	const failure = openai.failureOf(err);
	const described = failure && describeFailure(failure);
	return described
		? new PlanError(described, failure.code ?? `http_${failure.status}`, { cause: err })
		: err;
}

async function asPlan<T>(work: () => Promise<T>): Promise<T> {
	try {
		// Signed in, with the plan allowed and a token that works: said before anything is sent.
		await chatGptAccessToken();
		return await work();
	} catch (err) {
		throw planError(err);
	}
}

// --- the plan's models ---

/** A model the plan offers, as nolune needs it. */
export interface ChatGptModel {
	id: string;
	name: string;
	description: string | null;
	/** Whether ChatGPT offers it in model pickers; hidden ones work too. */
	listed: boolean;
	/** The reasoning efforts it takes (`low`… `xhigh`), when the catalog says. */
	efforts: string[];
	contextWindow: number | null;
}

function str(value: unknown): string | null {
	return typeof value === 'string' && value ? value : null;
}

function modelOf(entry: Record<string, unknown>): ChatGptModel | null {
	const id = str(entry.slug) ?? str(entry.id);
	if (!id) return null;
	const levels = entry.supported_reasoning_levels ?? entry.supported_reasoning_efforts;
	const efforts = Array.isArray(levels)
		? levels.flatMap((level: unknown) => {
				const effort =
					typeof level === 'string' ? level : str((level as Record<string, unknown>)?.effort);
				return effort ? [effort] : [];
			})
		: [];
	const window = entry.context_window ?? entry.max_context_window;
	return {
		id,
		name: str(entry.display_name) ?? id,
		description: str(entry.description),
		listed: (entry.visibility ?? 'list') === 'list',
		efforts,
		contextWindow: typeof window === 'number' && window > 0 ? window : null
	};
}

/** "0.160.0" as its numbers; null for a version of another kind (a pre-release, say). */
function versionParts(version: string): number[] | null {
	const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
	return match ? match.slice(1).map(Number) : null;
}

function isNewer(version: string, than: string): boolean {
	const a = versionParts(version);
	const b = versionParts(than);
	if (!a || !b) return false;
	const i = a.findIndex((part, j) => part !== b[j]);
	return i >= 0 && a[i] > b[i];
}

let releaseCache: { url: string; at: number; version: string } | null = null;

/**
 * The Codex version the model catalog is asked as. The catalog is Codex's: it shows a model only
 * to a client at or past the model's `minimal_client_version`, and without a version it answers
 * with an older list, so new models (gpt-6.1-sol, when it came) are missing. A new model comes
 * with the Codex release that knows it, so this is Codex's latest release, looked up on npm at
 * most hourly, and CODEX_VERSION when npm doesn't answer or says an older one.
 */
async function clientVersion(): Promise<string> {
	const url = codexReleaseUrl();
	if (releaseCache?.url === url && Date.now() - releaseCache.at < CODEX_RELEASE_TTL_MS) {
		return releaseCache.version;
	}
	let version = CODEX_VERSION;
	try {
		const res = await fetch(url, {
			headers: { accept: 'application/json' },
			signal: AbortSignal.timeout(CODEX_RELEASE_TIMEOUT_MS)
		});
		const latest = res.ok ? str(((await res.json()) as Record<string, unknown>).version) : null;
		if (latest && isNewer(latest, version)) version = latest;
	} catch {
		// The version nolune knows, until the next look.
	}
	releaseCache = { url, at: Date.now(), version };
	return version;
}

/** The catalog's answer, asked as Codex `version`, or with no version when null. */
async function askCatalog(token: string, version: string | null): Promise<Response> {
	const url = new URL(`${apiUrl()}/models`);
	if (version) url.searchParams.set('client_version', version);
	try {
		return await fetch(url, {
			headers: { authorization: `Bearer ${token}`, accept: 'application/json' },
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS)
		});
	} catch (err) {
		const why = err instanceof Error ? err.message : String(err);
		throw new PlanError(`Couldn't reach OpenAI for the ChatGPT plan's models (${why}).`);
	}
}

let catalogCache: { account: string; at: number; models: ChatGptModel[] } | null = null;

/**
 * The models the signed-in account's plan offers, in ChatGPT's order: its model catalog, which
 * answers `GET /models` with the plan's token (a `models` array, not the API's `data`), asked as
 * Codex's latest release (clientVersion).
 */
async function catalog(refresh = false): Promise<ChatGptModel[]> {
	const account = requireRegistration().clientId;
	if (
		!refresh &&
		catalogCache?.account === account &&
		Date.now() - catalogCache.at < CATALOG_TTL_MS
	) {
		return catalogCache.models;
	}
	const [token, version] = await Promise.all([chatGptAccessToken(), clientVersion()]);
	let res = await askCatalog(token, version);
	if (res.status === 400) {
		// Should OpenAI stop taking the version, the list without one beats none.
		await res.body?.cancel();
		res = await askCatalog(token, null);
	}
	const text = await res.text();
	let body: Record<string, unknown> = {};
	try {
		body = JSON.parse(text) as Record<string, unknown>;
	} catch {
		// not JSON
	}
	if (!res.ok) {
		const error = (body.error ?? {}) as Record<string, unknown>;
		const failure = {
			status: res.status,
			code: str(error.code),
			param: str(error.param),
			message: str(error.message) ?? str(body.detail) ?? (text.slice(0, 200) || res.statusText)
		};
		throw new PlanError(
			describeFailure(failure) ??
				`OpenAI said ${res.status} to the ChatGPT plan: ${failure.message}`,
			failure.code ?? `http_${res.status}`
		);
	}
	const entries = Array.isArray(body.models)
		? body.models
		: Array.isArray(body.data)
			? body.data
			: [];
	const models = (entries as Record<string, unknown>[]).flatMap((e) => modelOf(e) ?? []);
	catalogCache = { account, at: Date.now(), models };
	return models;
}

/** Every model the plan offers, hidden ones too: for `nolune chatgpt-plan models`. */
export function listChatGptModels(): Promise<ChatGptModel[]> {
	return asPlan(() => catalog(true));
}

/** The models ChatGPT offers in its pickers, in its order, for the admin page's. */
export async function listModels(): Promise<ModelChoice[]> {
	return (await listChatGptModels()).flatMap((m) =>
		m.listed
			? [{ id: m.id, name: m.name, description: m.description, contextWindow: m.contextWindow }]
			: []
	);
}

/**
 * Checks that someone is signed in with the plan allowed and that it offers the model. Its window
 * is the one the catalog lists, if any: the API's may not be the plan's, so it isn't guessed.
 */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const models = await asPlan(() => catalog(true));
	const found = models.find((m) => m.id === model);
	if (found) return found.contextWindow;
	const offered = models.filter((m) => m.listed).map((m) => m.id);
	throw new PlanError(
		`The ChatGPT plan has no model "${model}"${offered.length ? `. It has ${offered.join(', ')}` : ''}.`,
		'not_found'
	);
}

/**
 * The chat's effort, or the nearest below it that the model takes, as the catalog lists them.
 * As it is when the catalog doesn't say.
 */
async function effortFor(model: string, effort: Effort): Promise<Effort> {
	let efforts: string[] = [];
	try {
		efforts = (await catalog()).find((m) => m.id === model)?.efforts ?? [];
	} catch {
		// The request says what's wrong itself.
	}
	if (!efforts.length || efforts.includes(effort)) return effort;
	const lower = EFFORTS.slice(0, EFFORTS.indexOf(effort)).reverse();
	return (
		lower.find((e) => efforts.includes(e)) ?? EFFORTS.find((e) => efforts.includes(e)) ?? effort
	);
}

// --- requests ---

/** One model call of a chat, streamed (see models.ts). */
export function streamResponse(opts: {
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	messages: Message[];
	cacheKey: string;
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<OpenAI.Responses.Response> {
	return asPlan(async () =>
		openai.streamResponse(
			{ ...opts, effort: await effortFor(opts.model, opts.effort) },
			CHATGPT_PLAN
		)
	);
}

/** One short exchange, for chores like naming a chat (see models.ts). */
export function createResponse(opts: {
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<OpenAI.Responses.Response> {
	return asPlan(() => openai.createResponse(opts, CHATGPT_PLAN));
}

// --- the sign-in, as Models & keys and `nolune chatgpt-plan status` show it ---

export interface ChatGptPlanStatus extends PlanStatus {
	account: PlanAccount | null;
}

/**
 * Who's signed in with ChatGPT and what stops chats on the plan, from what nolune keeps. With
 * `check`, it also asks OpenAI, which refreshes the sign-in and lists the plan's models: nothing
 * is billed. Nothing needs installing, so `installed` is always true.
 */
export async function chatGptPlanStatus(
	opts: { check?: boolean } = {}
): Promise<ChatGptPlanStatus> {
	const r = activeRegistration();
	const account = r && (r.accessToken || r.refreshToken) ? planAccountOf(r) : null;
	const status = {
		path: null,
		installed: true,
		account,
		signedIn: account && describePlanAccount(account)
	};
	try {
		requireRegistration();
		if (opts.check) await catalog(true);
		return { ...status, problem: null };
	} catch (err) {
		const error = planError(err);
		// The check may have found that the sign-in ended.
		const now = activeRegistration();
		const still = !!now && !!(now.accessToken || now.refreshToken);
		return {
			...status,
			account: still ? account : null,
			signedIn: still ? status.signedIn : null,
			problem: error instanceof Error ? error.message : String(error)
		};
	}
}

/** Throws a PlanError unless someone is signed in with ChatGPT and OpenAI takes the sign-in. */
export async function checkChatGptPlan(): Promise<ChatGptPlanStatus> {
	const status = await chatGptPlanStatus({ check: true });
	if (status.problem) throw new PlanError(status.problem);
	return status;
}
