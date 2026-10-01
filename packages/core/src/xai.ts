import { createHash } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import { withoutUnreadable, type Message } from './format.ts';
import { EFFORTS, type Effort, type ModelChoice, type StreamEvent } from './models.ts';
import * as openai from './openai-chat.ts';

/*
 * Chats on xAI's Grok models, with an xAI API key. xAI's API is OpenAI's Responses API, so the
 * requests are openai-chat.ts's, with a client pointed at xAI (`XAI`, a ResponsesApi) and with
 * what xAI does differently:
 *
 * - encrypted reasoning goes back to the model that wrote it, as on OpenAI, whether or not the
 *   model takes an effort, and the prompt cache key is the chat, which also keeps it on the one
 *   of xAI's servers that has its cache (it's xAI's `x-grok-conv-id`);
 * - each model takes its own reasoning efforts, or none (`effortFor`), and no reasoning summary
 *   is asked for: xAI's models give theirs anyway;
 * - no Files API: pictures go inline, as JPEG or PNG, to the models that see them, and PDFs as
 *   their paths (xAI only searches a file sent with a request);
 * - errors come as `{ code, error }`, and a key xAI doesn't know is a 400, not a 401.
 *
 * Models, their windows, efforts and what they take besides text come from xAI's model lists.
 */

const REQUEST_TIMEOUT_MS = 60_000;
/** The model list changes rarely; it's asked for again after this long. */
const MODELS_TTL_MS = 60 * 60_000;

/** XAI_BASE_URL points it at a proxy or a stand-in server. */
export function xaiBaseUrl(): string {
	return (process.env.XAI_BASE_URL || 'https://api.x.ai/v1').replace(/\/+$/, '');
}

type Sdk = typeof import('openai');

/**
 * OpenAI's SDK, loaded on first use: the bundled CLI carries all of core, and most `nolune`
 * commands never call a model (see openai-chat.ts).
 */
let sdk: Sdk | undefined;

async function loadSdk(): Promise<Sdk> {
	return (sdk ??= await import('openai'));
}

type ErrorClass =
	| 'APIError'
	| 'APIUserAbortError'
	| 'APIConnectionError'
	| 'APIConnectionTimeoutError'
	| 'BadRequestError'
	| 'AuthenticationError'
	| 'PermissionDeniedError'
	| 'NotFoundError'
	| 'RateLimitError';

/** Whether `err` is one of the SDK's errors, which it can't be before the SDK was loaded. */
function isSdkError<K extends ErrorClass>(err: unknown, name: K): err is InstanceType<Sdk[K]> {
	return !!sdk && err instanceof sdk[name];
}

/** Something xAI's side can't do, in words for people. */
class XaiError extends Error {}

class MissingApiKeyError extends XaiError {
	constructor() {
		super(`No xAI API key. ${apiKeyHelp('xai')}`);
	}
}

/**
 * Errors from calls to xAI. Its SDK errors are the same classes as OpenAI's, so models.ts tells
 * them apart by this before asking openai-chat.ts.
 */
const ours = new WeakSet<object>();

async function tagged<T>(call: () => Promise<T>): Promise<T> {
	try {
		return await call();
	} catch (err) {
		if (err !== null && typeof err === 'object') ours.add(err);
		throw err;
	}
}

export function isXaiError(err: unknown): boolean {
	return err instanceof XaiError || (err !== null && typeof err === 'object' && ours.has(err));
}

function apiKey(): string {
	const found = configuredApiKey('xai');
	if (!found) throw new MissingApiKeyError();
	return found.key;
}

/** Whose calls these are, for what's learned from refusals. A hash, so the key isn't stored. */
function accountOf(key: string): string {
	return createHash('sha256').update(key).digest('hex').slice(0, 16);
}

let cached: { key: string; baseURL: string; client: OpenAI } | undefined;

async function getClient(): Promise<OpenAI> {
	const key = apiKey();
	const baseURL = xaiBaseUrl();
	const { OpenAI: Client } = await loadSdk();
	if (!cached || cached.key !== key || cached.baseURL !== baseURL) {
		// Never OpenAI's organization or project from the environment: they're for OpenAI's key.
		const client = new Client({ apiKey: key, baseURL, organization: null, project: null });
		cached = { key, baseURL, client };
	}
	return cached.client;
}

/** xAI's requests, through openai-chat.ts. */
export const XAI: openai.ResponsesApi = {
	provider: 'xai',
	client: getClient,
	account: () => accountOf(apiKey()),
	modelName: (model) => model,
	effort: (model, effort) => effortFor(model, effort)
};

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
	return tagged(() => openai.streamResponse(opts, XAI));
}

/**
 * Room for reasoning in a short exchange with a model that takes no effort: it reasons as much as
 * it likes, and xAI counts that within `max_output_tokens`, so a one-word answer (a title, auto
 * mode's verdict) could otherwise be cut off before it's written.
 */
const REASONING_ROOM = 16_000;

/** One short exchange, for chores like naming a chat (see models.ts). */
export function createResponse(opts: {
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<OpenAI.Responses.Response> {
	return tagged(async () => {
		const room = (await effortFor(opts.model, 'low')) === null ? REASONING_ROOM : 0;
		return openai.createResponse({ ...opts, maxTokens: opts.maxTokens + room }, XAI);
	});
}

// --- models ---

/** The picture formats xAI takes; others are converted first (models.ts, pictureTypes). */
export const PICTURE_TYPES = ['image/jpeg', 'image/png'] as const;

/** A model as nolune needs it, from xAI's two model lists. */
interface ModelInfo {
	id: string;
	/** Other names it answers to (`grok-4` for `grok-4-0709`). */
	aliases: string[];
	/** When xAI added it, in seconds. */
	created: number;
	/** Pictures, besides text. */
	pictures: boolean;
	contextWindow: number | null;
	/** The reasoning efforts it takes (`low`… `xhigh`); none for a model that takes none. */
	efforts: string[];
}

/** An entry of `GET /models` or `GET /language-models`: the fields nolune reads. */
interface Listed {
	id?: unknown;
	aliases?: unknown;
	created?: unknown;
	/** `/models` only. */
	context_length?: unknown;
	/** `/language-models` only. */
	input_modalities?: unknown;
	output_modalities?: unknown;
	capabilities?: { reasoning_effort?: unknown } | null;
}

function strings(value: unknown): string[] {
	return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : [];
}

let catalog: { key: string; at: number; models: Promise<ModelInfo[]> } | undefined;

/**
 * The models the key can chat with, kept for an hour (`fresh` asks again). `/language-models`
 * lists the ones that write text, with what they take besides it; `/models` their windows and
 * reasoning efforts. Multi-agent models are left out: they take none of nolune's tools.
 */
function catalogOf(fresh = false): Promise<ModelInfo[]> {
	const key = apiKey();
	if (fresh || !catalog || catalog.key !== key || Date.now() - catalog.at > MODELS_TTL_MS) {
		const models = tagged(async () => {
			const client = await getClient();
			const options = { timeout: REQUEST_TIMEOUT_MS };
			const [language, all] = await Promise.all([
				client.get<{ models?: Listed[] }>('/language-models', options),
				client.get<{ data?: Listed[] }>('/models', options)
			]);
			const details = new Map((all.data ?? []).map((m) => [m.id, m]));
			return (language.models ?? []).flatMap((m): ModelInfo[] => {
				if (typeof m.id !== 'string' || /multi-agent/.test(m.id)) return [];
				const outputs = strings(m.output_modalities);
				if (outputs.length && !outputs.includes('text')) return [];
				const more = details.get(m.id);
				const window = more?.context_length;
				return [
					{
						id: m.id,
						aliases: strings(m.aliases ?? more?.aliases),
						created: Number(m.created ?? more?.created) || 0,
						pictures: strings(m.input_modalities).includes('image'),
						contextWindow: typeof window === 'number' && window > 0 ? window : null,
						efforts: strings(
							m.capabilities?.reasoning_effort ?? more?.capabilities?.reasoning_effort
						)
					}
				];
			});
		});
		const entry = { key, at: Date.now(), models };
		catalog = entry;
		// A failed list isn't kept.
		models.catch(() => {
			if (catalog === entry) catalog = undefined;
		});
	}
	return catalog!.models;
}

/** The model by its id or one of its aliases. */
async function modelInfo(model: string, fresh = false): Promise<ModelInfo | undefined> {
	const models = await catalogOf(fresh);
	return models.find((m) => m.id === model) ?? models.find((m) => m.aliases.includes(model));
}

/**
 * The effort to send for the chat's: the chat's, or the nearest below it the model takes, or
 * above it when it takes none below; none for a model that takes none (it reasons all the same,
 * or not at all). For a model xAI didn't list, the chat's, at most `high`: a refusal is learned
 * (openai-chat.ts).
 */
async function effortFor(model: string, effort: Effort): Promise<Effort | null> {
	let info: ModelInfo | undefined;
	try {
		info = await modelInfo(model);
	} catch {
		// The request says what's wrong itself.
	}
	if (!info) return EFFORTS.indexOf(effort) > EFFORTS.indexOf('high') ? 'high' : effort;
	const takes = EFFORTS.filter((e) => info.efforts.includes(e));
	if (!takes.length) return null;
	if (takes.includes(effort)) return effort;
	const below = takes.filter((e) => EFFORTS.indexOf(e) < EFFORTS.indexOf(effort));
	return below.at(-1) ?? takes[0];
}

/**
 * What the model can be sent besides text, as xAI lists it: pictures to the models that see them.
 * PDFs go as their paths: xAI's models don't take a whole PDF, only a search through it.
 */
export async function modelInputs(model: string): Promise<{ pictures: boolean; pdfs: boolean }> {
	return { pictures: !!(await modelInfo(model))?.pictures, pdfs: false };
}

/** The messages as the model can take them: pictures it can't see and every PDF as notes. */
export function readableMessages(messages: Message[], model: string): Promise<Message[]> {
	return withoutUnreadable(messages, model, () => modelInputs(model));
}

/** Checks that the key can chat with the model, and says how large its window is. */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const info = await modelInfo(model, true);
	if (info) return info.contextWindow;
	if (/multi-agent/.test(model)) {
		throw new XaiError(`${model} takes no tools of nolune's, so it can't run commands.`);
	}
	const offered = (await catalogOf()).map((m) => m.id).slice(0, 8);
	throw new XaiError(
		`Model not found: xAI has no model "${model}" to chat with for this key${offered.length ? `. It has ${offered.join(', ')}` : ''}.`
	);
}

/** The models a preset can take, for the admin page, the newest first. */
export async function listModels(): Promise<ModelChoice[]> {
	const models = [...(await catalogOf(true))].sort((a, b) => b.created - a.created);
	return models.map((m) => ({
		id: m.id,
		name: null,
		description: null,
		contextWindow: m.contextWindow
	}));
}

// --- errors ---

/** The API's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		const body = err.error as { message?: unknown; error?: unknown } | string | undefined;
		if (typeof body === 'string' && body) return body;
		if (body && typeof body === 'object') {
			if (typeof body.message === 'string') return body.message;
			if (typeof body.error === 'string') return body.error;
		}
	}
	return openai.shortApiError(err);
}

/** xAI answers a key it doesn't know with a 400, "Incorrect API key provided". */
function rejectsKey(err: unknown): boolean {
	return isSdkError(err, 'BadRequestError') && /api key/i.test(shortApiError(err));
}

export function describeApiError(err: unknown): string {
	if (err instanceof XaiError) return err.message;
	if (isSdkError(err, 'AuthenticationError') || rejectsKey(err)) {
		return `xAI didn't accept the API key. ${apiKeyHelp('xai')}`;
	}
	if (isSdkError(err, 'PermissionDeniedError')) {
		// No credits left, a spending limit reached, or a key that isn't allowed this model.
		return `xAI turned the request down: ${shortApiError(err)} See the team's credits and limits at https://console.x.ai.`;
	}
	if (isSdkError(err, 'RateLimitError')) return `Rate limited by xAI: ${shortApiError(err)}`;
	if (isSdkError(err, 'NotFoundError')) return `Model not found: ${shortApiError(err)}`;
	if (isSdkError(err, 'APIConnectionTimeoutError')) return "xAI didn't answer in time.";
	if (isSdkError(err, 'APIConnectionError')) {
		// fetch says "fetch failed"; the reason (ECONNREFUSED, ENOTFOUND...) is in its cause.
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach xAI (${why}).`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `xAI API error ${err.status}: ${shortApiError(err)}`;
	}
	return `xAI: ${shortApiError(err)}`;
}
