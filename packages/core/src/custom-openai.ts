import type { OpenAI } from 'openai';
import { readConfig, updateConfig } from './config.ts';
import { withoutUnreadable, type Message } from './format.ts';
import type { ModelChoice } from './models.ts';
import * as openai from './openai-chat.ts';

/*
 * Custom OpenAI: any server that speaks OpenAI's API, at an address the admin gives, with a key
 * when it wants one: Ollama, LM Studio, vLLM, LiteLLM... It's a provider of its own: presets run
 * chats on its models, and memory search can take its embeddings (memory-embeddings.ts). The rest
 * of btw calls it through models.ts.
 *
 * Chats are OpenAI's (openai-chat.ts): the Responses API through OpenAI's SDK, with a client
 * pointed at the server. What differs: the requests leave out what only OpenAI has (encrypted
 * reasoning, its prompt cache key, levels above `high`), and a model the server refuses reasoning
 * settings for gets none. There's no Files API, and nothing says which of a server's models see
 * pictures or read PDFs, so those go as their paths, which the agent opens with commands. Its
 * models are what it lists, with a window only when the list gives one, so a preset's is set by
 * hand or stays unknown.
 */

export const CUSTOM_OPENAI_LABEL = 'Custom OpenAI';
/** Used when config.json has none, like the API keys' variables. */
export const CUSTOM_OPENAI_ENV = { url: 'CUSTOM_OPENAI_BASE_URL', key: 'CUSTOM_OPENAI_API_KEY' };

const REQUEST_TIMEOUT_MS = 60_000;
const CHECK_TIMEOUT_MS = 20_000;

function saved(): { url?: string; key?: string } {
	try {
		const config = readConfig();
		return { url: config.customOpenaiUrl, key: config.customOpenaiApiKey };
	} catch {
		// not set up yet: only the environment can have one
		return {};
	}
}

/** An address as the SDK takes it: without a trailing slash. */
export function normalizeServerUrl(value: string): string {
	return value.trim().replace(/\/+$/, '');
}

/** Whether `value` can be a server's address. */
export function isServerUrl(value: string): boolean {
	return /^https?:\/\/[^\s/]+/i.test(value.trim());
}

/**
 * The server in use: config.json's, else the environment's. Null without an address. The
 * environment's key goes only to the environment's server, or to any when it names none, so a
 * server saved in btw never gets another's key.
 */
export function customOpenai(): { url: string; key: string | null } | null {
	const config = saved();
	const envUrl = process.env[CUSTOM_OPENAI_ENV.url];
	const url = config.url || envUrl;
	if (!url) return null;
	const address = normalizeServerUrl(url);
	const envKeyFits = !envUrl || normalizeServerUrl(envUrl) === address;
	const key = config.key || (envKeyFits ? process.env[CUSTOM_OPENAI_ENV.key] : null) || null;
	return { url: address, key };
}

export interface CustomOpenaiStatus {
	url: string | null;
	/** Where the address in use comes from. */
	source: 'config' | 'env' | null;
	hasKey: boolean;
	/** The last four characters of the key in use, never the key. */
	hint: string | null;
	/** Whether the environment names a server, which is used when none is saved. */
	envSet: boolean;
}

/** For Models & keys and `btw config`: the address and whether there's a key, never the key. */
export function customOpenaiStatus(): CustomOpenaiStatus {
	const config = saved();
	const server = customOpenai();
	return {
		url: server?.url ?? null,
		source: config.url ? 'config' : server ? 'env' : null,
		hasKey: !!server?.key,
		hint: server?.key && server.key.length >= 16 ? server.key.slice(-4) : null,
		envSet: !!process.env[CUSTOM_OPENAI_ENV.url]
	};
}

/**
 * Saves the server. `key`: a new one, null to take it away, undefined to keep the one saved for
 * the same address (a form that never showed it).
 */
export function saveCustomOpenai(url: string, key: string | null | undefined): void {
	const address = normalizeServerUrl(url);
	updateConfig((c) => {
		const kept = key === undefined && c.customOpenaiUrl === address ? c.customOpenaiApiKey : null;
		c.customOpenaiUrl = address;
		const next = key ?? kept;
		if (next) c.customOpenaiApiKey = next;
		else delete c.customOpenaiApiKey;
	});
}

export function removeCustomOpenai(): void {
	updateConfig((c) => {
		delete c.customOpenaiUrl;
		delete c.customOpenaiApiKey;
	});
}

/**
 * Something the server's side can't do, in words for people. `reason`, from checkCustomOpenai:
 * `key` when it turned the key down or wants one, `unreachable` when it didn't answer.
 */
export class CustomOpenaiError extends Error {
	readonly reason: 'key' | 'unreachable' | 'other';

	constructor(message: string, reason: CustomOpenaiError['reason'] = 'other') {
		super(message);
		this.reason = reason;
	}
}

/** fetch says "fetch failed"; the reason is in its cause (ECONNREFUSED, ENOTFOUND...). */
function networkError(err: unknown): string {
	if (!(err instanceof Error)) return String(err);
	if (err.name === 'TimeoutError') return 'no answer';
	const cause = err.cause as { code?: unknown; message?: unknown } | undefined;
	if (typeof cause?.code === 'string') return cause.code;
	if (typeof cause?.message === 'string') return cause.message;
	return err.message;
}

/**
 * Asks the server for its models, as saving it on Models & keys or with `btw key set` does: the
 * ids it serves, with a warning when it answers but doesn't list any. Throws a CustomOpenaiError
 * when it can't be reached or turns the key down.
 */
export async function checkCustomOpenai(
	url: string,
	key: string | null
): Promise<{ models: string[]; warning: string | null }> {
	if (!isServerUrl(url)) {
		throw new CustomOpenaiError('The address must start with http:// or https://.');
	}
	let res: Response;
	try {
		res = await fetch(`${normalizeServerUrl(url)}/models`, {
			headers: key ? { authorization: `Bearer ${key}` } : {},
			signal: AbortSignal.timeout(CHECK_TIMEOUT_MS)
		});
	} catch (err) {
		throw new CustomOpenaiError(`Couldn't reach ${url} (${networkError(err)}).`, 'unreachable');
	}
	if (res.status === 401 || res.status === 403) {
		throw new CustomOpenaiError(
			key ? "The server didn't accept the key." : 'The server wants a key.',
			'key'
		);
	}
	if (!res.ok) {
		return {
			models: [],
			warning: `It answered ${res.status} when asked for its models; type the models' ids yourself.`
		};
	}
	const body = (await res.json().catch(() => null)) as { data?: { id?: unknown }[] } | null;
	const models = (body?.data ?? []).flatMap((m) => (typeof m?.id === 'string' ? [m.id] : []));
	return { models, warning: models.length ? null : "It doesn't list any models yet." };
}

// --- chats ---

type Sdk = typeof import('openai');
/** OpenAI's SDK, loaded on first use (see openai-chat.ts). */
let sdk: Sdk | undefined;
let cached: { url: string; key: string; client: OpenAI } | undefined;

function server(): { url: string; key: string | null } {
	const found = customOpenai();
	if (!found) {
		throw new CustomOpenaiError(
			`No ${CUSTOM_OPENAI_LABEL} server yet. An admin can add its address under Models & keys in btw, or with \`btw key set custom-openai <url>\`.`
		);
	}
	return found;
}

async function getClient(): Promise<OpenAI> {
	const { url, key } = server();
	// A server that takes no key still gets one: the SDK wants it.
	const apiKey = key ?? 'none';
	if (!cached || cached.url !== url || cached.key !== apiKey) {
		const { OpenAI: Client } = (sdk ??= await import('openai'));
		cached = { url, key: apiKey, client: new Client({ apiKey, baseURL: url }) };
	}
	return cached.client;
}

/**
 * Errors from calls to the server. They're OpenAI's SDK's classes, and the same code's, so
 * models.ts tells them apart by this before asking openai-chat.ts.
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

export function isCustomOpenaiError(err: unknown): boolean {
	return (
		err instanceof CustomOpenaiError || (err !== null && typeof err === 'object' && ours.has(err))
	);
}

const api: openai.ResponsesApi = {
	provider: 'custom-openai',
	client: getClient,
	account: () => server().url
};

/** One model call, streamed, as OpenAI's (see models.ts). */
export function streamResponse(
	opts: Parameters<typeof openai.streamResponse>[0]
): Promise<OpenAI.Responses.Response> {
	return tagged(() => openai.streamResponse(opts, api));
}

/** One short exchange, not streamed, as OpenAI's (see models.ts). */
export function createResponse(
	opts: Parameters<typeof openai.createResponse>[0]
): Promise<OpenAI.Responses.Response> {
	return tagged(() => openai.createResponse(opts, api));
}

/** A server without a reasoning parser leaves a thinking model's thoughts in its text. */
export function withoutThinking(text: string): string {
	return text.replace(/^\s*<think>[\s\S]*?<\/think>\s*/, '');
}

/** Pictures and PDFs go as their paths: nothing says which of a server's models take them. */
export async function modelInputs(): Promise<{ pictures: boolean; pdfs: boolean }> {
	return { pictures: false, pdfs: false };
}

export function readableMessages(messages: Message[], model: string): Promise<Message[]> {
	return withoutUnreadable(messages, model, modelInputs);
}

interface ModelInfo {
	id: string;
	created?: number | null;
	/** vLLM says it; most servers don't. */
	max_model_len?: number | null;
	context_length?: number | null;
}

async function serverModels(): Promise<ModelInfo[]> {
	return tagged(async () => {
		const client = await getClient();
		const all: ModelInfo[] = [];
		for await (const model of client.models.list({ timeout: REQUEST_TIMEOUT_MS })) {
			if (typeof model?.id === 'string') all.push(model as ModelInfo);
		}
		return all;
	});
}

function windowOf(info: ModelInfo | undefined): number | null {
	const n = info?.max_model_len ?? info?.context_length;
	return typeof n === 'number' && n > 0 ? n : null;
}

/** The server's models, for the admin page, as it lists them. */
export async function listModels(): Promise<ModelChoice[]> {
	return (await serverModels()).map((m) => ({
		id: m.id,
		name: null,
		description: null,
		contextWindow: windowOf(m)
	}));
}

/**
 * Checks that the server has the model, when it lists its models, and says how large its window
 * is when it says so. Whether it calls tools shows at the chat's first reply.
 */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const models = await serverModels();
	if (!models.length) return null;
	const info = models.find((m) => m.id === model);
	if (!info) {
		const some = models
			.slice(0, 8)
			.map((m) => m.id)
			.join(', ');
		throw new CustomOpenaiError(
			`Model not found: the ${CUSTOM_OPENAI_LABEL} server has no model "${model}". It serves ${some}${models.length > 8 ? '…' : ''}.`
		);
	}
	return windowOf(info);
}

// --- errors ---

type ErrorClass =
	| 'APIError'
	| 'APIUserAbortError'
	| 'APIConnectionError'
	| 'APIConnectionTimeoutError'
	| 'AuthenticationError';

function isSdkError<K extends ErrorClass>(err: unknown, name: K): err is InstanceType<Sdk[K]> {
	return !!sdk && err instanceof sdk[name];
}

function address(): string {
	return customOpenai()?.url ?? 'the server';
}

export function describeApiError(err: unknown): string {
	if (err instanceof CustomOpenaiError) return err.message;
	if (isSdkError(err, 'AuthenticationError')) {
		return `The ${CUSTOM_OPENAI_LABEL} server at ${address()} didn't accept the key. An admin can change it under Models & keys in btw, or with \`btw key set custom-openai\`.`;
	}
	if (isSdkError(err, 'APIConnectionTimeoutError')) {
		return `The ${CUSTOM_OPENAI_LABEL} server at ${address()} didn't answer in time.`;
	}
	if (isSdkError(err, 'APIConnectionError')) {
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach the ${CUSTOM_OPENAI_LABEL} server at ${address()} (${why}). Is it running?`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `${CUSTOM_OPENAI_LABEL} error ${err.status}: ${shortApiError(err)}`;
	}
	return `${CUSTOM_OPENAI_LABEL}: ${shortApiError(err)}`;
}

/** The server's own message, without the status and JSON around it. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		const body = err.error as { message?: unknown } | string | undefined;
		if (typeof body === 'string' && body.trim()) return body.trim().slice(0, 300);
		if (body && typeof body === 'object' && typeof body.message === 'string') return body.message;
	}
	return err instanceof Error ? err.message : String(err);
}

export function isAbortError(err: unknown): boolean {
	return isSdkError(err, 'APIUserAbortError');
}
