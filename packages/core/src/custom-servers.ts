import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import * as anthropic from './anthropic.ts';
import { readConfig, updateConfig } from './config.ts';
import { withoutUnreadable, type Message } from './format.ts';
import type { ModelChoice } from './models.ts';
import * as openai from './openai-chat.ts';

/*
 * The family's own model servers: Ollama, LM Studio, oMLX, vLLM, llama.cpp's server, LiteLLM...
 * Each has a name the admin gives (`local`, `gpu`), an address, and a key when it wants one.
 * Two providers run chats on them, each in its API's format through its SDK: Custom OpenAI is
 * OpenAI's code (openai-chat.ts, the Responses API) and Custom Anthropic is Anthropic's
 * (anthropic.ts, the Messages API), with a client pointed at the server. A server that speaks
 * both (Ollama, LM Studio, oMLX) is added once and used either way. Memory search can take a
 * server's embeddings (memory-embeddings.ts). The rest of btw calls them through models.ts.
 *
 * A custom preset's model is `<server>/<model>` (`gpu/qwen3:32b`): server names have no slash,
 * so the first one ends it, and the model's own id may have more. Chats copy it from their
 * preset like any model id, so a chat stays on the server it started on.
 *
 * The requests leave out what only OpenAI and Anthropic have (see each module). There's no Files
 * API, and nothing says which of a server's models see pictures or read PDFs, so those go as
 * their paths, which the agent opens with commands. Its models are what it lists, with a window
 * only when the list gives one, so a preset's is set by hand or stays unknown.
 */

export const CUSTOM_PROVIDERS = ['custom-openai', 'custom-anthropic'] as const;
export type CustomProvider = (typeof CUSTOM_PROVIDERS)[number];

export const CUSTOM_LABELS: Record<CustomProvider, string> = {
	'custom-openai': 'Custom OpenAI',
	'custom-anthropic': 'Custom Anthropic'
};

export function isCustomProvider(value: string): value is CustomProvider {
	return (CUSTOM_PROVIDERS as readonly string[]).includes(value);
}

const CHECK_TIMEOUT_MS = 20_000;

/** As config.json keeps one. */
export interface Server {
	name: string;
	url: string;
	key?: string;
}

function saved(): Server[] {
	try {
		return readConfig().servers ?? [];
	} catch {
		// not set up yet
		return [];
	}
}

/** Letters, digits, `-` and `_`: it starts a model's id, so never a slash. */
export function isServerName(value: string): boolean {
	return /^[a-z0-9][a-z0-9_-]{0,31}$/i.test(value);
}

/** An address as it's kept: without a trailing slash. */
export function normalizeServerUrl(value: string): string {
	return value.trim().replace(/\/+$/, '');
}

/** Whether `value` can be a server's address. */
export function isServerUrl(value: string): boolean {
	return /^https?:\/\/[^\s/]+/i.test(value.trim());
}

/**
 * Where OpenAI's API is on a server: its address, or `/v1` under it when it's only a host
 * (`http://localhost:11434` → `http://localhost:11434/v1`).
 */
export function openaiUrl(url: string): string {
	const address = normalizeServerUrl(url);
	return new URL(address).pathname.replace(/\/+$/, '') ? address : `${address}/v1`;
}

/** Where Anthropic's is: the SDK adds `/v1/messages`, so without a `/v1` at the end. */
export function anthropicUrl(url: string): string {
	return normalizeServerUrl(url).replace(/\/v1$/, '');
}

/** A default name for a server at `url`: `local` on this computer, else its host's first word. */
export function suggestServerName(url: string): string {
	let host: string;
	try {
		host = new URL(normalizeServerUrl(url)).hostname;
	} catch {
		return 'local';
	}
	if (/^(localhost|127\.|\[?::1\]?$)/.test(host)) return 'local';
	const word = host.split('.')[0].replace(/[^a-z0-9_-]/gi, '-');
	return isServerName(word) ? word.toLowerCase() : 'server';
}

export function findServer(name: string): Server | undefined {
	return saved().find((s) => s.name.toLowerCase() === name.toLowerCase());
}

export interface ServerStatus {
	name: string;
	url: string;
	hasKey: boolean;
	/** The last four characters of its key, never the key. */
	hint: string | null;
}

/** For Models & keys and `btw server list`: never a key. */
export function listServers(): ServerStatus[] {
	return saved().map((s) => ({
		name: s.name,
		url: s.url,
		hasKey: !!s.key,
		hint: s.key && s.key.length >= 16 ? s.key.slice(-4) : null
	}));
}

/**
 * Adds a server, or changes the one of that name. `key`: a new one, null to take it away,
 * undefined to keep the one saved for the same address (a form that never showed it).
 */
export function saveServer(name: string, url: string, key: string | null | undefined): void {
	if (!isServerName(name)) {
		throw new CustomServerError(
			'A server name is letters, digits, - and _ (up to 32), like local or gpu.'
		);
	}
	const address = normalizeServerUrl(url);
	updateConfig((c) => {
		const servers = c.servers ?? [];
		const old = servers.find((s) => s.name.toLowerCase() === name.toLowerCase());
		const kept = key === undefined && old?.url === address ? old.key : null;
		const next: Server = { name: old?.name ?? name, url: address };
		if (key ?? kept) next.key = (key ?? kept)!;
		c.servers = old ? servers.map((s) => (s === old ? next : s)) : [...servers, next];
	});
}

/** Chats and presets on it stop working until they're moved to another model. */
export function removeServer(name: string): void {
	updateConfig((c) => {
		c.servers = (c.servers ?? []).filter((s) => s.name.toLowerCase() !== name.toLowerCase());
		if (!c.servers.length) delete c.servers;
	});
}

/** A custom model's server and its id there: `gpu/qwen3:32b` → gpu, qwen3:32b. */
export function splitModel(model: string): { server: string; model: string } {
	const slash = model.indexOf('/');
	return slash > 0
		? { server: model.slice(0, slash), model: model.slice(slash + 1) }
		: { server: '', model };
}

/** The server a custom model runs on, or why there's none. */
function serverOf(model: string): Server {
	const { server: name } = splitModel(model);
	const server = name ? findServer(name) : undefined;
	if (server) return server;
	const names = saved().map((s) => s.name);
	const hint = names.length
		? `Servers: ${names.join(', ')}.`
		: 'An admin can add one under Models & keys in btw, or with `btw server add`.';
	throw new CustomServerError(
		name
			? `No server named "${name}". ${hint}`
			: `"${model}" doesn't say which server it's on: write it as <server>/<model>. ${hint}`
	);
}

/**
 * Something about a server, in words for people. `reason`, from checkServer: `key` when it
 * turned the key down or wants one, `unreachable` when it didn't answer.
 */
export class CustomServerError extends Error {
	readonly reason: 'key' | 'unreachable' | 'other';

	constructor(message: string, reason: CustomServerError['reason'] = 'other') {
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

/** Both ways servers take a key: OpenAI's (a bearer token) and Anthropic's. */
function authHeaders(key: string | null | undefined): Record<string, string> {
	return key ? { authorization: `Bearer ${key}`, 'x-api-key': key } : {};
}

interface ModelInfo {
	id: string;
	/** vLLM says it; most servers don't. */
	max_model_len?: number | null;
	context_length?: number | null;
}

/** GET /v1/models, which servers answer in OpenAI's shape whichever APIs they speak. */
async function fetchModels(url: string, key: string | null | undefined): Promise<ModelInfo[]> {
	let res: Response;
	try {
		res = await fetch(`${openaiUrl(url)}/models`, {
			headers: authHeaders(key),
			signal: AbortSignal.timeout(CHECK_TIMEOUT_MS)
		});
	} catch (err) {
		throw new CustomServerError(`Couldn't reach ${url} (${networkError(err)}).`, 'unreachable');
	}
	if (res.status === 401 || res.status === 403) {
		throw new CustomServerError(
			key ? "The server didn't accept the key." : 'The server wants a key.',
			'key'
		);
	}
	if (!res.ok) {
		throw new CustomServerError(`It answered ${res.status} when asked for its models.`);
	}
	const body = (await res.json().catch(() => null)) as { data?: unknown } | null;
	const data = Array.isArray(body?.data) ? (body.data as ModelInfo[]) : [];
	return data.filter((m) => typeof m?.id === 'string');
}

/**
 * Asks a server for its models, as saving it does: the ids it serves, with a warning when it
 * answers without any. Throws a CustomServerError when it can't be reached or turns the key down.
 */
export async function checkServer(
	url: string,
	key: string | null
): Promise<{ models: string[]; warning: string | null }> {
	if (!isServerUrl(url)) {
		throw new CustomServerError('The address must start with http:// or https://.');
	}
	let models: ModelInfo[];
	try {
		models = await fetchModels(url, key);
	} catch (err) {
		if (err instanceof CustomServerError && err.reason === 'other') {
			return { models: [], warning: `${err.message} Type the models' ids yourself.` };
		}
		throw err;
	}
	return {
		models: models.map((m) => m.id),
		warning: models.length ? null : "It doesn't list any models yet."
	};
}

// --- the SDKs' clients ---

type OpenaiSdk = typeof import('openai');
type AnthropicSdk = typeof import('@anthropic-ai/sdk');
/** Loaded on first use, like the providers' own (see openai-chat.ts). */
let openaiSdk: OpenaiSdk | undefined;
let anthropicSdk: AnthropicSdk | undefined;
const clients = new Map<string, OpenAI | Anthropic>();

/**
 * OpenAI's SDK at the server. Everything it would read from the environment is given, so
 * OPENAI_API_KEY and the organization never reach another server. A server that takes no key
 * still gets one: the SDK wants it.
 */
async function openaiClient(server: Server): Promise<OpenAI> {
	const id = `openai ${server.url} ${server.key ?? ''}`;
	if (!clients.has(id)) {
		const { OpenAI: Client } = (openaiSdk ??= await import('openai'));
		clients.set(
			id,
			new Client({
				apiKey: server.key || 'none',
				baseURL: openaiUrl(server.url),
				organization: null,
				project: null
			})
		);
	}
	return clients.get(id) as OpenAI;
}

/**
 * Anthropic's SDK at the server, likewise: ANTHROPIC_API_KEY and saved credentials never reach
 * it. The key goes both ways servers take it (`x-api-key`, and a bearer token for Ollama's).
 */
async function anthropicClient(server: Server): Promise<Anthropic> {
	const id = `anthropic ${server.url} ${server.key ?? ''}`;
	if (!clients.has(id)) {
		const { Anthropic: Client } = (anthropicSdk ??= await import('@anthropic-ai/sdk'));
		clients.set(
			id,
			new Client({
				apiKey: server.key || 'none',
				authToken: server.key || null,
				baseURL: anthropicUrl(server.url)
			})
		);
	}
	return clients.get(id) as Anthropic;
}

/** The server and provider of errors from calls to a server, for describing them. */
const ours = new WeakMap<object, { server: Server | null; provider: CustomProvider }>();

async function tagged<T>(
	provider: CustomProvider,
	model: string,
	call: (server: Server) => Promise<T>
): Promise<T> {
	let server: Server | null = null;
	try {
		server = serverOf(model);
		return await call(server);
	} catch (err) {
		if (err !== null && typeof err === 'object') ours.set(err, { server, provider });
		throw err;
	}
}

/**
 * Errors from calls to a server. They're the SDKs' classes, and the providers' own code's, so
 * models.ts tells them apart by this before asking the providers.
 */
export function isCustomServerError(err: unknown): boolean {
	return (
		err instanceof CustomServerError || (err !== null && typeof err === 'object' && ours.has(err))
	);
}

// --- chats ---

/** OpenAI's code at the server (openai-chat.ts). */
function responsesApi(server: Server): openai.ResponsesApi {
	return {
		provider: 'custom-openai',
		client: () => openaiClient(server),
		account: () => server.url,
		modelName: (model) => splitModel(model).model
	};
}

/** Anthropic's code at the server (anthropic.ts). */
function messagesApi(server: Server): anthropic.MessagesApi {
	return {
		provider: 'custom-anthropic',
		client: () => anthropicClient(server),
		modelName: (model) => splitModel(model).model
	};
}

/** One Custom OpenAI call, streamed, as OpenAI's (see models.ts). */
export function streamResponse(
	opts: Parameters<typeof openai.streamResponse>[0]
): Promise<OpenAI.Responses.Response> {
	return tagged('custom-openai', opts.model, (server) =>
		openai.streamResponse(opts, responsesApi(server))
	);
}

/** One short Custom OpenAI exchange, not streamed, as OpenAI's. */
export function createResponse(
	opts: Parameters<typeof openai.createResponse>[0]
): Promise<OpenAI.Responses.Response> {
	return tagged('custom-openai', opts.model, (server) =>
		openai.createResponse(opts, responsesApi(server))
	);
}

/** One Custom Anthropic call, streamed, as Anthropic's (see models.ts). */
export function streamMessage(
	opts: Parameters<typeof anthropic.streamTurn>[0]
): Promise<Anthropic.Message> {
	return tagged('custom-anthropic', opts.model, (server) =>
		anthropic.streamTurn(opts, messagesApi(server))
	);
}

/** One short Custom Anthropic exchange, not streamed, as Anthropic's. */
export function createMessage(
	opts: Parameters<typeof anthropic.createMessage>[0]
): Promise<Anthropic.Message> {
	return tagged('custom-anthropic', opts.model, (server) =>
		anthropic.createMessage(opts, messagesApi(server))
	);
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

// --- models ---

function windowOf(info: ModelInfo | undefined): number | null {
	const n = info?.max_model_len ?? info?.context_length;
	return typeof n === 'number' && n > 0 ? n : null;
}

/** A server's models, for the admin page, as it lists them, each as `<server>/<model>`. */
export function listModels(provider: CustomProvider, name: string): Promise<ModelChoice[]> {
	return tagged(provider, `${name}/`, async (server) =>
		(await fetchModels(server.url, server.key)).map((m) => ({
			id: `${server.name}/${m.id}`,
			name: m.id,
			description: null,
			contextWindow: windowOf(m)
		}))
	);
}

/**
 * Checks that the server has the model, when it lists its models, and says how large its window
 * is when it says so. Whether it calls tools shows at the chat's first reply.
 */
export function fetchContextWindow(
	provider: CustomProvider,
	model: string
): Promise<number | null> {
	return tagged(provider, model, async (server) => {
		const id = splitModel(model).model;
		const models = await fetchModels(server.url, server.key);
		if (!models.length) return null;
		const info = models.find((m) => m.id === id);
		if (!info) {
			const some = models
				.slice(0, 8)
				.map((m) => m.id)
				.join(', ');
			throw new CustomServerError(
				`Model not found: the server "${server.name}" has no model "${id}". It serves ${some}${models.length > 8 ? '…' : ''}.`
			);
		}
		return windowOf(info);
	});
}

// --- errors ---

type ErrorClass =
	| 'APIError'
	| 'APIUserAbortError'
	| 'APIConnectionError'
	| 'APIConnectionTimeoutError'
	| 'AuthenticationError';

/** One of either SDK's errors, which it can't be before that SDK was loaded. */
function isSdkError(err: unknown, name: ErrorClass): err is Error & { status?: number } {
	return (
		(!!openaiSdk && err instanceof openaiSdk[name]) ||
		(!!anthropicSdk && err instanceof anthropicSdk[name])
	);
}

export function describeApiError(err: unknown): string {
	if (err instanceof CustomServerError) return err.message;
	const tag = err !== null && typeof err === 'object' ? ours.get(err) : undefined;
	const label = CUSTOM_LABELS[tag?.provider ?? 'custom-openai'];
	const server = tag?.server
		? `the server "${tag.server.name}" at ${tag.server.url}`
		: 'the server';
	if (isSdkError(err, 'AuthenticationError')) {
		return `${server[0].toUpperCase()}${server.slice(1)} didn't accept the key. An admin can change it under Models & keys in btw, or with \`btw server add\`.`;
	}
	if (isSdkError(err, 'APIConnectionTimeoutError')) {
		return `${server[0].toUpperCase()}${server.slice(1)} didn't answer in time.`;
	}
	if (isSdkError(err, 'APIConnectionError')) {
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach ${server} (${why}). Is it running?`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `${label} error ${err.status}: ${shortApiError(err)}`;
	}
	return `${label}: ${shortApiError(err)}`;
}

/** The server's own message, without the status and JSON around it. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		// OpenAI's shape is { message }, Anthropic's { error: { message } }; some send text.
		const body = (err as { error?: unknown }).error as
			{ message?: unknown; error?: { message?: unknown } } | string | undefined;
		if (typeof body === 'string' && body.trim()) return body.trim().slice(0, 300);
		if (body && typeof body === 'object') {
			if (typeof body.message === 'string') return body.message;
			if (typeof body.error?.message === 'string') return body.error.message;
		}
	}
	return err instanceof Error ? err.message : String(err);
}

export function isAbortError(err: unknown): boolean {
	return isSdkError(err, 'APIUserAbortError');
}
