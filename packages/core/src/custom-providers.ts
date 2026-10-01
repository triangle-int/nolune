import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import * as anthropic from './anthropic.ts';
import { readConfig, updateConfig } from './config.ts';
import { withoutUnreadable, type Message } from './format.ts';
import type { ModelChoice } from './models.ts';
import * as openai from './openai-chat.ts';

/*
 * Custom providers: the family's own model servers (Ollama, LM Studio, oMLX, vLLM, llama.cpp's
 * server, LiteLLM...), each added under Models & keys as a provider of its own: a name ("Ollama",
 * "GPU box"), the API it speaks (OpenAI's or Anthropic's), an address, and a key when it wants
 * one. Chats on one run on OpenAI's code (openai-chat.ts, the Responses API) or Anthropic's
 * (anthropic.ts, the Messages API), with that SDK pointed at it: in nolune's words, the
 * `custom-openai` or `custom-anthropic` provider. A server that speaks both can be added once for
 * each. Memory search can take an OpenAI one's embeddings (memory-embeddings.ts). The rest of nolune
 * calls them through models.ts.
 *
 * Each gets an id from its name when it's added (`gpu-box`), which never changes, so its name
 * can: a custom preset's model is `<id>/<model>` (`gpu-box/qwen3:32b`). Ids have no slash, so
 * the first one ends it, and the model's own id may have more. Chats copy it from their preset
 * like any model id, so a chat stays on the custom provider it started on.
 *
 * The requests leave out what only OpenAI and Anthropic have (see each module). There's no Files
 * API, and nothing says which of a server's models see pictures or read PDFs, so those go as
 * their paths, which the agent opens with commands. Its models are what it lists, with a window
 * only when the list gives one, so a preset's is set by hand or stays unknown.
 */

/** The API a custom provider speaks. */
export type CustomApi = 'openai' | 'anthropic';
export const CUSTOM_APIS: CustomApi[] = ['openai', 'anthropic'];

/** Whose API it is, for people. */
const API_NAMES: Record<CustomApi, string> = { openai: "OpenAI's", anthropic: "Anthropic's" };

/** nolune's providers for chats on custom providers, one for each API. */
export const CUSTOM_PROVIDERS = ['custom-openai', 'custom-anthropic'] as const;
export type CustomProvider = (typeof CUSTOM_PROVIDERS)[number];

export const CUSTOM_LABELS: Record<CustomProvider, string> = {
	'custom-openai': 'Custom OpenAI',
	'custom-anthropic': 'Custom Anthropic'
};

export function isCustomProvider(value: string): value is CustomProvider {
	return (CUSTOM_PROVIDERS as readonly string[]).includes(value);
}

/** The provider chats on a custom provider of this API run as. */
export function providerFor(api: CustomApi): CustomProvider {
	return api === 'anthropic' ? 'custom-anthropic' : 'custom-openai';
}

function apiOf(provider: CustomProvider): CustomApi {
	return provider === 'custom-anthropic' ? 'anthropic' : 'openai';
}

/** As config.json keeps one. */
export interface CustomProviderConfig {
	/** From its name when it was added; the start of its presets' models. */
	id: string;
	name: string;
	api: CustomApi;
	url: string;
	key?: string;
}

/** What `nolune preset add --provider` and embeddings take besides a custom provider's id. */
const RESERVED = [
	'anthropic',
	'openai',
	'openrouter',
	'xai',
	'claude-plan',
	'chatgpt-plan',
	'nolune-plan',
	'custom',
	'custom-openai',
	'custom-anthropic',
	'auto',
	'off'
];

const CHECK_TIMEOUT_MS = 20_000;

function saved(): CustomProviderConfig[] {
	try {
		return readConfig().customProviders ?? [];
	} catch {
		// not set up yet
		return [];
	}
}

/** An id from a name: lower case, its words joined by `-` (`GPU box` → `gpu-box`). */
export function customProviderId(name: string): string {
	return name
		.normalize('NFKD')
		.replace(/\p{M}/gu, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 32)
		.replace(/-+$/, '');
}

/** An address as it's kept: without a trailing slash. */
export function normalizeProviderUrl(value: string): string {
	return value.trim().replace(/\/+$/, '');
}

/** Whether `value` can be an address. */
export function isProviderUrl(value: string): boolean {
	return /^https?:\/\/[^\s/]+/i.test(value.trim());
}

/**
 * Where OpenAI's API is on a server: its address, or `/v1` under it when it's only a host
 * (`http://localhost:11434` → `http://localhost:11434/v1`).
 */
export function openaiUrl(url: string): string {
	const address = normalizeProviderUrl(url);
	return new URL(address).pathname.replace(/\/+$/, '') ? address : `${address}/v1`;
}

/** Where Anthropic's is: the SDK adds `/v1/messages`, so without a `/v1` at the end. */
export function anthropicUrl(url: string): string {
	return normalizeProviderUrl(url).replace(/\/v1$/, '');
}

/** By id or name, as `nolune preset add --provider` and the forms give it. */
export function findCustomProvider(idOrName: string): CustomProviderConfig | undefined {
	const wanted = idOrName.trim().toLowerCase();
	const all = saved();
	return all.find((p) => p.id === wanted) ?? all.find((p) => p.name.toLowerCase() === wanted);
}

export interface CustomProviderStatus {
	id: string;
	name: string;
	api: CustomApi;
	url: string;
	hasKey: boolean;
	/** The last four characters of its key, never the key. */
	hint: string | null;
}

/** For Models & keys and `nolune provider list`: never a key. */
export function listCustomProviders(): CustomProviderStatus[] {
	return saved().map((p) => ({
		id: p.id,
		name: p.name,
		api: p.api,
		url: p.url,
		hasKey: !!p.key,
		hint: p.key && p.key.length >= 16 ? p.key.slice(-4) : null
	}));
}

/** Its name as it's kept: trimmed, with single spaces. */
function tidyName(name: string): string {
	return name.trim().replace(/\s+/g, ' ');
}

/**
 * Why `name` can't be a custom provider's (`id`: the one being changed), for the forms to say
 * before its server is asked; null when it can.
 */
export function customProviderNameProblem(name: string, id?: string): string | null {
	const tidy = tidyName(name);
	if (!tidy || tidy.length > 40) {
		return 'Give it a name of up to 40 characters, like Ollama or GPU box.';
	}
	if (!customProviderId(tidy)) return 'Its name needs a letter or a digit.';
	if (!id && RESERVED.includes(customProviderId(tidy))) {
		return `There's already a provider called ${tidy}.`;
	}
	const taken = saved().find((p) => p.id !== id && p.name.toLowerCase() === tidy.toLowerCase());
	return taken ? `There's already a provider called ${taken.name}.` : null;
}

/**
 * Adds a custom provider (without `id`), or changes one: its name, address and key. Its id and
 * API stay, since its presets' models and chats depend on them. `key`: a new one, null to take it
 * away, undefined to keep the one saved for the same address (a form that never showed it).
 * Returns its id.
 */
export function saveCustomProvider(input: {
	id?: string;
	name: string;
	api?: CustomApi;
	url: string;
	key: string | null | undefined;
}): string {
	const all = saved();
	const old = input.id ? all.find((p) => p.id === input.id) : undefined;
	if (input.id && !old) throw new CustomProviderError(`No custom provider "${input.id}".`);
	const problem = customProviderNameProblem(input.name, old?.id);
	if (problem) throw new CustomProviderError(problem);
	const name = tidyName(input.name);
	let id = old?.id ?? customProviderId(name);
	// One renamed since may have the id this name makes.
	for (let n = 2; !old && all.some((p) => p.id === id); n++) {
		id = `${customProviderId(name)}-${n}`;
	}
	const api = old?.api ?? input.api ?? 'openai';
	const url = normalizeProviderUrl(input.url);
	const kept = input.key === undefined && old?.url === url ? old.key : null;
	const next: CustomProviderConfig = { id, name, api, url };
	if (input.key ?? kept) next.key = (input.key ?? kept)!;
	updateConfig((c) => {
		const list = c.customProviders ?? [];
		c.customProviders = old ? list.map((p) => (p.id === id ? next : p)) : [...list, next];
	});
	return id;
}

/** Chats and presets on it stop working until they're moved to another model. */
export function removeCustomProvider(id: string): void {
	updateConfig((c) => {
		c.customProviders = (c.customProviders ?? []).filter((p) => p.id !== id);
		if (!c.customProviders.length) delete c.customProviders;
	});
}

/** A custom model's provider id and its model there: `gpu-box/qwen3:32b` → gpu-box, qwen3:32b. */
export function splitModel(model: string): { provider: string; model: string } {
	const slash = model.indexOf('/');
	return slash > 0
		? { provider: model.slice(0, slash), model: model.slice(slash + 1) }
		: { provider: '', model };
}

/** The custom provider a model runs on, or why there's none. */
function providerOf(model: string): CustomProviderConfig {
	const { provider: id } = splitModel(model);
	const found = id ? saved().find((p) => p.id === id) : undefined;
	if (found) return found;
	const names = saved().map((p) => p.name);
	const hint = names.length
		? `Custom providers: ${names.join(', ')}.`
		: 'An admin can add one under Models & keys in nolune, or with `nolune provider add`.';
	throw new CustomProviderError(
		id
			? `No custom provider "${id}". ${hint}`
			: `"${model}" doesn't say which custom provider it's on: write it as <provider>/<model>. ${hint}`
	);
}

/**
 * Something about a custom provider, in words for people. `reason`, from checkCustomProvider:
 * `key` when it turned the key down or wants one, `unreachable` when it didn't answer.
 */
export class CustomProviderError extends Error {
	readonly reason: 'key' | 'unreachable' | 'other';

	constructor(message: string, reason: CustomProviderError['reason'] = 'other') {
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
		throw new CustomProviderError(`Couldn't reach ${url} (${networkError(err)}).`, 'unreachable');
	}
	if (res.status === 401 || res.status === 403) {
		throw new CustomProviderError(
			key ? "The server didn't accept the key." : 'The server wants a key.',
			'key'
		);
	}
	if (!res.ok) {
		throw new CustomProviderError(`It answered ${res.status} when asked for its models.`);
	}
	const body = (await res.json().catch(() => null)) as { data?: unknown } | null;
	const data = Array.isArray(body?.data) ? (body.data as ModelInfo[]) : [];
	return data.filter((m) => typeof m?.id === 'string');
}

/**
 * Asks a server for its models, as saving a custom provider does: the ids it serves, with a
 * warning when it answers without any. Throws a CustomProviderError when it can't be reached or
 * turns the key down.
 */
export async function checkCustomProvider(
	url: string,
	key: string | null
): Promise<{ models: string[]; warning: string | null }> {
	if (!isProviderUrl(url)) {
		throw new CustomProviderError('The address must start with http:// or https://.');
	}
	let models: ModelInfo[];
	try {
		models = await fetchModels(url, key);
	} catch (err) {
		if (err instanceof CustomProviderError && err.reason === 'other') {
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
async function openaiClient(provider: CustomProviderConfig): Promise<OpenAI> {
	const id = `openai ${provider.url} ${provider.key ?? ''}`;
	if (!clients.has(id)) {
		const { OpenAI: Client } = (openaiSdk ??= await import('openai'));
		clients.set(
			id,
			new Client({
				apiKey: provider.key || 'none',
				baseURL: openaiUrl(provider.url),
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
async function anthropicClient(provider: CustomProviderConfig): Promise<Anthropic> {
	const id = `anthropic ${provider.url} ${provider.key ?? ''}`;
	if (!clients.has(id)) {
		const { Anthropic: Client } = (anthropicSdk ??= await import('@anthropic-ai/sdk'));
		clients.set(
			id,
			new Client({
				apiKey: provider.key || 'none',
				authToken: provider.key || null,
				baseURL: anthropicUrl(provider.url)
			})
		);
	}
	return clients.get(id) as Anthropic;
}

/** The custom provider of errors from calls to one, for describing them. */
const ours = new WeakMap<object, CustomProviderConfig | null>();

async function tagged<T>(
	api: CustomApi | null,
	model: string,
	call: (provider: CustomProviderConfig) => Promise<T>
): Promise<T> {
	let provider: CustomProviderConfig | null = null;
	try {
		provider = providerOf(model);
		if (api && provider.api !== api) {
			throw new CustomProviderError(
				`${provider.name} speaks ${API_NAMES[provider.api]} API, not ${API_NAMES[api]}.`
			);
		}
		return await call(provider);
	} catch (err) {
		if (err !== null && typeof err === 'object') ours.set(err, provider);
		throw err;
	}
}

/**
 * Errors from calls to a custom provider. They're the SDKs' classes, and the providers' own
 * code's, so models.ts tells them apart by this before asking the providers.
 */
export function isCustomProviderError(err: unknown): boolean {
	return (
		err instanceof CustomProviderError || (err !== null && typeof err === 'object' && ours.has(err))
	);
}

// --- chats ---

/** OpenAI's code at the server (openai-chat.ts). */
function responsesApi(provider: CustomProviderConfig): openai.ResponsesApi {
	return {
		provider: 'custom-openai',
		client: () => openaiClient(provider),
		account: () => provider.url,
		modelName: (model) => splitModel(model).model
	};
}

/** Anthropic's code at the server (anthropic.ts). */
function messagesApi(provider: CustomProviderConfig): anthropic.MessagesApi {
	return {
		provider: 'custom-anthropic',
		client: () => anthropicClient(provider),
		modelName: (model) => splitModel(model).model
	};
}

/** One call on a custom provider of OpenAI's API, streamed, as OpenAI's (see models.ts). */
export function streamResponse(
	opts: Parameters<typeof openai.streamResponse>[0]
): Promise<OpenAI.Responses.Response> {
	return tagged('openai', opts.model, (provider) =>
		openai.streamResponse(opts, responsesApi(provider))
	);
}

/** One short exchange on one, not streamed, as OpenAI's. */
export function createResponse(
	opts: Parameters<typeof openai.createResponse>[0]
): Promise<OpenAI.Responses.Response> {
	return tagged('openai', opts.model, (provider) =>
		openai.createResponse(opts, responsesApi(provider))
	);
}

/** One call on a custom provider of Anthropic's API, streamed, as Anthropic's (see models.ts). */
export function streamMessage(
	opts: Parameters<typeof anthropic.streamTurn>[0]
): Promise<Anthropic.Message> {
	return tagged('anthropic', opts.model, (provider) =>
		anthropic.streamTurn(opts, messagesApi(provider))
	);
}

/** One short exchange on one, not streamed, as Anthropic's. */
export function createMessage(
	opts: Parameters<typeof anthropic.createMessage>[0]
): Promise<Anthropic.Message> {
	return tagged('anthropic', opts.model, (provider) =>
		anthropic.createMessage(opts, messagesApi(provider))
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

/** A custom provider's models, for the admin page, as it lists them, each as `<id>/<model>`. */
export function listModels(id: string): Promise<ModelChoice[]> {
	return tagged(null, `${id}/`, async (provider) =>
		(await fetchModels(provider.url, provider.key)).map((m) => ({
			id: `${provider.id}/${m.id}`,
			name: m.id,
			description: null,
			contextWindow: windowOf(m)
		}))
	);
}

/**
 * Checks that the custom provider has the model, when it lists its models, and says how large
 * its window is when it says so. Whether it calls tools shows at the chat's first reply.
 */
export function fetchContextWindow(on: CustomProvider, model: string): Promise<number | null> {
	return tagged(apiOf(on), model, async (provider) => {
		const id = splitModel(model).model;
		const models = await fetchModels(provider.url, provider.key);
		if (!models.length) return null;
		const info = models.find((m) => m.id === id);
		if (!info) {
			const some = models
				.slice(0, 8)
				.map((m) => m.id)
				.join(', ');
			throw new CustomProviderError(
				`Model not found: ${provider.name} has no model "${id}". It serves ${some}${models.length > 8 ? '…' : ''}.`
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
	if (err instanceof CustomProviderError) return err.message;
	const provider = err !== null && typeof err === 'object' ? ours.get(err) : undefined;
	const at = provider ? `${provider.name} at ${provider.url}` : null;
	if (isSdkError(err, 'AuthenticationError')) {
		return `${at ?? 'The custom provider'} didn't accept the key. An admin can change it under Models & keys in nolune.`;
	}
	if (isSdkError(err, 'APIConnectionTimeoutError')) {
		return `${at ?? 'The custom provider'} didn't answer in time.`;
	}
	if (isSdkError(err, 'APIConnectionError')) {
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach ${at ?? 'the custom provider'} (${why}). Is it running?`;
	}
	const name = provider?.name ?? 'Custom provider';
	if (isSdkError(err, 'APIError') && err.status) {
		return `${name} error ${err.status}: ${shortApiError(err)}`;
	}
	return `${name}: ${shortApiError(err)}`;
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
