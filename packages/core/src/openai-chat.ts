import { createHash } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { Usage } from './conversations.ts';
import {
	heldElsewhere,
	heldElsewhereNote,
	portableReply,
	unresolved,
	type Block,
	type Message,
	type ToolResultBlock
} from './format.ts';
import type { Effort, ModelChoice, StreamEvent } from './models.ts';
import { openaiBaseUrl } from './openai.ts';

/*
 * Chats on OpenAI's models, through the Responses API and OpenAI's SDK, and its Files API for
 * pictures and PDFs. The rest of nolune calls it through models.ts. A custom provider
 * (custom-providers.ts) can speak the same API, so its chats go through the same code with its
 * own client (a `ResponsesApi`), leaving out what only OpenAI has; so do chats on the ChatGPT
 * plan (chatgpt-plan.ts), with the plan's token and what the plan asks of a request.
 *
 * Requests are stateless (`store: false`): like Anthropic's, every call sends the whole
 * transcript, so nothing depends on OpenAI keeping a conversation. A reply is stored as the
 * output items the API returned, its reasoning included in encrypted form, and sent back as it
 * came. nolune's own blocks (people's messages, command results; format.ts) are turned into input
 * items here, the same way on every call, so the request prefix stays byte-identical and OpenAI's
 * automatic prompt cache keeps serving it.
 */

type InputItem = OpenAI.Responses.ResponseInputItem;
type InputPart = OpenAI.Responses.ResponseInputContent;
type OutputPart = OpenAI.Responses.ResponseFunctionCallOutputItem;

const UPLOAD_TIMEOUT_MS = 5 * 60_000;
const REQUEST_TIMEOUT_MS = 60_000;

type Sdk = typeof import('openai');

/**
 * The SDK, loaded on first use. The bundled CLI carries all of core, and loading the SDK when
 * it starts would slow down every `nolune` command the agent runs, which almost never call OpenAI.
 */
let sdk: Sdk | undefined;

async function loadSdk(): Promise<Sdk> {
	return (sdk ??= await import('openai'));
}

type ErrorClass =
	| 'OpenAIError'
	| 'APIError'
	| 'APIUserAbortError'
	| 'APIConnectionError'
	| 'APIConnectionTimeoutError'
	| 'BadRequestError'
	| 'AuthenticationError'
	| 'NotFoundError'
	| 'RateLimitError';

/** Whether `err` is one of the SDK's errors, which it can't be before the SDK was loaded. */
function isSdkError<K extends ErrorClass>(err: unknown, name: K): err is InstanceType<Sdk[K]> {
	return !!sdk && err instanceof sdk[name];
}

let cached: { key: string; baseURL: string; client: OpenAI } | undefined;

class MissingApiKeyError extends Error {
	constructor() {
		super(`No OpenAI API key. ${apiKeyHelp('openai')}`);
	}
}

/** A reply that failed or broke off after the request itself went through. */
class ReplyError extends Error {
	/** The failure's code, when the response said (`subscription_sharing_usage_limit_exceeded`...). */
	readonly code: string | null;
	constructor(message: string, code: string | null = null) {
		super(message);
		this.code = code;
	}
}

export function isOpenAIError(err: unknown): boolean {
	return (
		isSdkError(err, 'OpenAIError') || err instanceof MissingApiKeyError || err instanceof ReplyError
	);
}

function apiKey(): string {
	const found = configuredApiKey('openai');
	if (!found) throw new MissingApiKeyError();
	return found.key;
}

/** OPENAI_BASE_URL points it at a proxy or a compatible server, as for pictures (openai.ts). */
async function getClient(): Promise<OpenAI> {
	const key = apiKey();
	const baseURL = openaiBaseUrl();
	const { OpenAI: Client } = await loadSdk();
	if (!cached || cached.key !== key || cached.baseURL !== baseURL) {
		cached = { key, baseURL, client: new Client({ apiKey: key, baseURL }) };
	}
	return cached.client;
}

/**
 * Where Responses API calls go: OpenAI, a custom provider (custom-providers.ts), or OpenAI on the
 * ChatGPT plan (chatgpt-plan.ts). A custom provider gets the same requests without what only
 * OpenAI has: encrypted reasoning, its prompt cache key and reasoning levels above `high`. The
 * plan gets its tools in nolune's namespace, and its short exchanges streamed.
 */
export interface ResponsesApi {
	provider: 'openai' | 'custom-openai' | 'chatgpt-plan';
	client(): Promise<OpenAI>;
	/** Whose calls these are, for what's learned from refusals: a hash or an address, never a key. */
	account(): string;
	/** The model's id where it runs: a custom provider's without its id before it. */
	modelName(model: string): string;
}

const OPENAI: ResponsesApi = {
	provider: 'openai',
	client: getClient,
	account: () => accountOf(apiKey()),
	modelName: (model) => model
};

// --- nolune's format as OpenAI takes it ---

type Stored = { type?: unknown } & Record<string, unknown>;

/** What OpenAI returned in a reply, which goes back as it is. */
const OUTPUT_TYPES = new Set(['message', 'reasoning', 'function_call']);

/** A text, picture or PDF block as an input content part; one `provider` can't open, as a note. */
function inputPart(block: Block, provider: ResponsesApi['provider']): InputPart | null {
	if (block.type === 'text') return { type: 'input_text', text: block.text };
	if (block.type !== 'image' && block.type !== 'pdf') return null;
	if (heldElsewhere(block, provider)) {
		return { type: 'input_text', text: heldElsewhereNote(block).text };
	}
	const { source } = block;
	if (source.type === 'media') unresolved(block);
	if (block.type === 'image') {
		if (source.type === 'uploaded') {
			return { type: 'input_image', file_id: source.fileId, detail: 'auto' };
		}
		const url = `data:${source.mime};base64,${source.data}`;
		return { type: 'input_image', image_url: url, detail: 'auto' };
	}
	if (source.type === 'uploaded') return { type: 'input_file', file_id: source.fileId };
	const data = `data:${source.mime};base64,${source.data}`;
	return { type: 'input_file', filename: block.name || 'document.pdf', file_data: data };
}

function toolOutput(
	content: ToolResultBlock['content'],
	provider: ResponsesApi['provider']
): string | OutputPart[] {
	if (typeof content === 'string') return content;
	return content.flatMap((b) => (inputPart(b, provider) as OutputPart | null) ?? []);
}

/** A reply of this model's, as the API returned it: its output items, reasoning included. */
function nativeItems(content: unknown[]): InputItem[] {
	const items: InputItem[] = [];
	for (const b of content as Stored[]) {
		if (b.type === 'text' && typeof b.text === 'string' && b.text) {
			// A reply nolune wrote itself, from before nolune's own format.
			items.push({ role: 'assistant', content: b.text });
		} else if (b.type === 'tool_use') {
			// A row that didn't record who wrote it (tests).
			items.push({
				type: 'function_call',
				call_id: String(b.id),
				name: String(b.name),
				arguments: JSON.stringify(b.input ?? {})
			});
		} else if (b.type === 'reasoning' && !b.encrypted_content) {
			continue;
		} else if (typeof b.type === 'string' && OUTPUT_TYPES.has(b.type)) {
			items.push(b as unknown as InputItem);
		}
	}
	return items;
}

/**
 * A conversation's messages as the Responses API's `input` for `model` on `provider`. Replies it
 * wrote go back as they came, except reasoning without its encrypted content, which can't be read
 * back without `store` (a custom provider's never has any). Replies from another model or
 * provider (the conversation switched), and nolune's own, go as their text and calls: reasoning goes
 * back only to the model that wrote it.
 */
export function toResponsesInput(
	messages: Message[],
	model: string,
	provider: ResponsesApi['provider'] = 'openai'
): InputItem[] {
	const input: InputItem[] = [];
	for (const m of messages) {
		if (m.role === 'assistant') {
			const native = m.native;
			if (
				native &&
				(native.provider === null || (native.provider === provider && native.model === model))
			) {
				input.push(...nativeItems(native.content));
				continue;
			}
			for (const b of portableReply(m.blocks)) {
				input.push(
					b.type === 'text'
						? { role: 'assistant', content: b.text }
						: {
								type: 'function_call',
								call_id: b.id,
								name: b.name,
								arguments: JSON.stringify(b.input ?? {})
							}
				);
			}
			continue;
		}
		// Results go on their own; what's around them stays in order, as user messages.
		let parts: InputPart[] = [];
		const flush = () => {
			if (parts.length) input.push({ role: 'user', content: parts });
			parts = [];
		};
		for (const b of m.blocks) {
			if (b.type === 'tool_result') {
				flush();
				input.push({
					type: 'function_call_output',
					call_id: b.callId,
					output: toolOutput(b.content, provider)
				});
			} else {
				const part = inputPart(b, provider);
				if (part) parts.push(part);
			}
		}
		flush();
	}
	return provider === 'chatgpt-plan' ? input.map(forPlan) : input;
}

/** The namespace nolune's tools are in on the ChatGPT plan, which wants function tools in one. */
export const TOOL_NAMESPACE = 'nolune';

/**
 * An input item as the ChatGPT plan takes it: calls in nolune's namespace, as its tools are, also
 * those of replies from another model or from before (when Codex ran the plan, whose replies'
 * messages carry ids of Codex's own, which go as their text).
 */
function forPlan(item: InputItem): InputItem {
	const stored = item as Stored;
	if (stored.type === 'function_call' && !stored.namespace) {
		return { ...item, namespace: TOOL_NAMESPACE } as InputItem;
	}
	if (
		stored.type === 'message' &&
		stored.role === 'assistant' &&
		!(typeof stored.id === 'string' && stored.id.startsWith('msg_'))
	) {
		const parts = (Array.isArray(stored.content) ? stored.content : []) as Stored[];
		const text = parts
			.map((part) => (part.type === 'refusal' ? part.refusal : part.text))
			.filter((t): t is string => typeof t === 'string')
			.join('');
		return { role: 'assistant', content: text };
	}
	return item;
}

/** A tool as nolune saves it (Anthropic's format) as a function tool. Not strict: `cwd` is optional. */
function functionTool(tool: Anthropic.Tool): OpenAI.Responses.FunctionTool {
	return {
		type: 'function',
		name: tool.name,
		description: tool.description,
		parameters: tool.input_schema,
		strict: false
	};
}

/** nolune's tools as `api` takes them: in nolune's namespace on the ChatGPT plan. */
function toolsFor(api: ResponsesApi, tools: Anthropic.Tool[]): OpenAI.Responses.Tool[] {
	if (api.provider !== 'chatgpt-plan') return tools.map(functionTool);
	if (!tools.length) return [];
	return [
		{
			type: 'namespace',
			name: TOOL_NAMESPACE,
			description: "nolune's tools.",
			tools: tools.map(functionTool)
		}
	];
}

/** GPT-4 models, and the chat-tuned ones ChatGPT uses, take no reasoning settings. */
export function supportsReasoning(model: string): boolean {
	return !/^(gpt-3|gpt-4|chatgpt-)/.test(model) && !/-chat/.test(model);
}

// --- a turn ---

/** Files and cached prompts belong to the key's project. A hash, so the key itself isn't stored. */
function accountOf(key: string): string {
	return createHash('sha256').update(key).digest('hex').slice(0, 16);
}

/**
 * Accounts that can't have reasoning summaries (they need a verified organization), learned
 * from the first refusal. Their replies still reason; the chat just can't show it.
 */
const noSummaries = new Set<string>();

function refusesSummaries(err: unknown): boolean {
	return (
		isSdkError(err, 'BadRequestError') && /summar/i.test(err.message) && /verif/i.test(err.message)
	);
}

/**
 * Models of a custom provider that refused reasoning settings (a model that doesn't reason, on a
 * server that says so), by address and model, learned from the first refusal.
 */
const noReasoning = new Set<string>();

function refusesReasoning(err: unknown): boolean {
	return (
		isSdkError(err, 'APIError') &&
		(err.status === 400 || err.status === 422) &&
		/reason|think|effort/i.test(err.message)
	);
}

/**
 * Makes a request with what the account and model take, learned from refusals: `summaries`, and
 * on a custom provider `reasoning` at all. A refusal is learned and the request made again.
 */
async function withRefusals<T>(
	api: ResponsesApi,
	model: string,
	request: (takes: { summaries: boolean; reasoning: boolean }) => Promise<T>
): Promise<T> {
	// A custom provider's client comes from custom-providers.ts: its errors are checked with this
	// module's SDK.
	await loadSdk();
	const account = api.account();
	const takes = () => ({
		summaries: !noSummaries.has(account),
		reasoning: supportsReasoning(api.modelName(model)) && !noReasoning.has(`${account} ${model}`)
	});
	try {
		return await request(takes());
	} catch (err) {
		if (!noSummaries.has(account) && refusesSummaries(err)) noSummaries.add(account);
		else if (api.provider === 'custom-openai' && takes().reasoning && refusesReasoning(err)) {
			noReasoning.add(`${account} ${model}`);
		} else throw err;
		return request(takes());
	}
}

/** OpenAI's levels above `high` are its own; a custom provider gets `high` for them. */
function effortFor(api: ResponsesApi, effort: Effort): Effort {
	return api.provider !== 'custom-openai' || !['xhigh', 'max'].includes(effort) ? effort : 'high';
}

/**
 * Reads the stream, telling `onEvent` about each block as it grows, and returns the response. Its
 * output is the last event's, or, when that leaves it out (the ChatGPT plan's route does), the
 * items the stream finished one by one.
 */
async function readStream(
	stream: AsyncIterable<OpenAI.Responses.ResponseStreamEvent>,
	onEvent: (event: StreamEvent) => void
): Promise<OpenAI.Responses.Response> {
	const finished: OpenAI.Responses.ResponseOutputItem[] = [];
	for await (const event of stream) {
		switch (event.type) {
			case 'response.output_item.added': {
				const { item, output_index: index } = event;
				if (item.type === 'message')
					onEvent({ type: 'block_start', index, block: { type: 'text' } });
				else if (item.type === 'reasoning') {
					onEvent({ type: 'block_start', index, block: { type: 'thinking' } });
				} else if (item.type === 'function_call') {
					onEvent({ type: 'block_start', index, block: { type: 'tool', id: item.call_id } });
				}
				break;
			}
			// Text, a refusal, OpenAI's reasoning summary, or a server's reasoning in full (vLLM).
			case 'response.output_text.delta':
			case 'response.refusal.delta':
			case 'response.reasoning_summary_text.delta':
			case 'response.reasoning_text.delta':
				onEvent({ type: 'delta', index: event.output_index, text: event.delta });
				break;
			case 'response.reasoning_summary_part.added':
				// Parts are paragraphs, as format.ts joins them.
				if (event.summary_index > 0) {
					onEvent({ type: 'delta', index: event.output_index, text: '\n\n' });
				}
				break;
			case 'response.output_item.done':
				finished[event.output_index] = event.item;
				break;
			case 'response.completed':
			case 'response.incomplete': {
				const { response } = event;
				if (!response.output?.length) response.output = finished.filter(Boolean);
				// Leaving the loop closes the stream.
				return response;
			}
			case 'response.failed':
				throw new ReplyError(
					event.response.error?.message || 'The reply failed.',
					event.response.error?.code ?? null
				);
		}
	}
	throw new ReplyError('The reply ended before it was complete.');
}

/**
 * One model call, streamed. See models.ts for what stays fixed between calls. `api`: OpenAI, or
 * a custom provider.
 */
export async function streamResponse(
	opts: {
		model: string;
		effort: Effort;
		system: string;
		tools: Anthropic.Tool[];
		messages: Message[];
		cacheKey: string;
		signal: AbortSignal;
		onEvent: (event: StreamEvent) => void;
	},
	api: ResponsesApi = OPENAI
): Promise<OpenAI.Responses.Response> {
	const client = await api.client();
	// OpenAI's own, with a key or on the ChatGPT plan.
	const openai = api.provider !== 'custom-openai';
	const stream = await withRefusals(api, opts.model, (takes) =>
		client.responses.create(
			{
				model: api.modelName(opts.model),
				instructions: opts.system,
				input: toResponsesInput(opts.messages, opts.model, api.provider),
				tools: toolsFor(api, opts.tools),
				store: false,
				stream: true,
				...(takes.reasoning
					? {
							reasoning: {
								effort: effortFor(api, opts.effort),
								...(takes.summaries ? { summary: 'auto' as const } : {})
							},
							...(openai ? { include: ['reasoning.encrypted_content' as const] } : {})
						}
					: {}),
				...(openai ? { prompt_cache_key: opts.cacheKey } : {})
			},
			{ signal: opts.signal }
		)
	);
	return readStream(stream, opts.onEvent);
}

/**
 * One short exchange, not streamed, at low effort (see models.ts). The ChatGPT plan takes only
 * streamed requests, without `max_output_tokens`, and its input as a list: it gets one of those,
 * read to its end.
 */
export async function createResponse(
	opts: {
		model: string;
		system: string;
		input: string;
		maxTokens: number;
		timeoutMs: number;
	},
	api: ResponsesApi = OPENAI
): Promise<OpenAI.Responses.Response> {
	const client = await api.client();
	if (api.provider === 'chatgpt-plan') {
		const signal = AbortSignal.timeout(opts.timeoutMs);
		const stream = await withRefusals(api, opts.model, (takes) =>
			client.responses.create(
				{
					model: api.modelName(opts.model),
					instructions: opts.system,
					input: [{ role: 'user', content: opts.input }],
					store: false,
					stream: true,
					...(takes.reasoning ? { reasoning: { effort: 'low' as const } } : {})
				},
				{ signal }
			)
		);
		return readStream(stream, () => {});
	}
	return withRefusals(api, opts.model, (takes) =>
		client.responses.create(
			{
				model: api.modelName(opts.model),
				instructions: opts.system,
				input: opts.input,
				max_output_tokens: opts.maxTokens,
				store: false,
				...(takes.reasoning ? { reasoning: { effort: 'low' as const } } : {})
			},
			{ timeout: opts.timeoutMs }
		)
	);
}

/** In Anthropic's words, as nolune stores it: see ModelReply in models.ts. */
export function stopReason(response: {
	status?: string;
	output?: readonly unknown[];
	incomplete_details?: { reason?: string } | null;
}): string {
	if (response.status === 'incomplete') {
		return response.incomplete_details?.reason === 'content_filter' ? 'refusal' : 'max_tokens';
	}
	const output = (response.output ?? []) as Stored[];
	if (output.some((item) => item.type === 'function_call')) return 'tool_use';
	const refused = output.some(
		(item) =>
			item.type === 'message' &&
			Array.isArray(item.content) &&
			(item.content as Stored[]).some((part) => part.type === 'refusal')
	);
	return refused ? 'refusal' : 'end_turn';
}

/** OpenAI counts cached tokens inside `input_tokens`; nolune counts them apart, as Anthropic does. */
export function summarizeUsage(
	usage:
		| {
				input_tokens?: number;
				input_tokens_details?: { cached_tokens?: number; cache_write_tokens?: number } | null;
				output_tokens?: number;
		  }
		| null
		| undefined
): Usage {
	const cacheRead = usage?.input_tokens_details?.cached_tokens ?? 0;
	const cacheWrite = usage?.input_tokens_details?.cache_write_tokens ?? 0;
	return {
		input: Math.max(0, (usage?.input_tokens ?? 0) - cacheRead - cacheWrite),
		cacheRead,
		cacheWrite,
		output: usage?.output_tokens ?? 0
	};
}

// --- files, models, errors ---

/** Keeps names short and plain; the model never needs them, the Files API only stores them. */
function uploadName(name: string): string {
	// eslint-disable-next-line no-control-regex
	const clean = name.replace(/[<>:"|?*\\/\x00-\x1f]/g, '_').trim();
	return clean.slice(-200) || 'file';
}

/** The Files API, as provider-files.ts uses it (see FileStore there). */
export const openaiFiles = {
	account(): string {
		return accountOf(apiKey());
	},
	async upload(data: Buffer, name: string, mime: string): Promise<string> {
		const client = await getClient();
		const file = await (await loadSdk()).toFile(data, uploadName(name), { type: mime });
		// Pictures are uploaded for vision, PDFs as the model's input.
		const purpose = mime.startsWith('image/') ? 'vision' : 'user_data';
		return (await client.files.create({ file, purpose }, { timeout: UPLOAD_TIMEOUT_MS })).id;
	},
	/** False once the file was deleted; other failures throw. */
	async exists(fileId: string): Promise<boolean> {
		const client = await getClient();
		try {
			await client.files.retrieve(fileId, { timeout: REQUEST_TIMEOUT_MS });
			return true;
		} catch (err) {
			if (isSdkError(err, 'NotFoundError')) return false;
			throw err;
		}
	},
	/** Resolves when the file is gone, also when it already was. */
	async remove(fileId: string): Promise<void> {
		const client = await getClient();
		try {
			await client.files.delete(fileId, { timeout: REQUEST_TIMEOUT_MS });
		} catch (err) {
			if (!isSdkError(err, 'NotFoundError')) throw err;
		}
	}
};

export async function countDocumentTokens(model: string, fileId: string): Promise<number> {
	const client = await getClient();
	const count = await client.responses.inputTokens.count(
		{ model, input: [{ role: 'user', content: [{ type: 'input_file', file_id: fileId }] }] },
		{ timeout: REQUEST_TIMEOUT_MS }
	);
	return count.input_tokens;
}

/** Every flagship model since GPT-5.4 has this window, the Pro ones included. */
const FLAGSHIP_CONTEXT_WINDOW = 1_050_000;

/** "gpt-5.6-sol" → 5, 6 and "-sol"; null for a name of another kind. */
function gptVersion(model: string): { major: number; minor: number; suffix: string } | null {
	const match = /^gpt-(\d+)(?:\.(\d+))?(-.*)?$/.exec(model);
	if (!match) return null;
	return { major: Number(match[1]), minor: Number(match[2] ?? 0), suffix: match[3] ?? '' };
}

/**
 * The model's context window when nolune knows it: OpenAI's models API doesn't say. Flagships
 * since GPT-5.4 (gpt-5.4, gpt-5.5-pro, gpt-5.6-terra, gpt-6-astra, and their dated snapshots)
 * have 1,050,000 tokens. Other models (mini, nano, codex, older ones) may have much less, and a
 * window set too large would let a conversation grow past what the model takes, for good, so
 * they get none.
 */
export function knownContextWindow(model: string): number | null {
	const version = gptVersion(model);
	if (!version) return null;
	const { major, minor, suffix } = version;
	if (major < 5 || (major === 5 && minor < 4)) return null;
	// The tiers' names, Pro, and a snapshot's date; any other suffix may be a smaller model.
	const flagship = /^(-(astra|sol|terra|luna))?(-pro)?(-\d{4}-\d{2}-\d{2})?$/.test(suffix);
	return flagship ? FLAGSHIP_CONTEXT_WINDOW : null;
}

/** Checks that the model exists, and says how large its window is when nolune knows it. */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const client = await getClient();
	await client.models.retrieve(model, { timeout: REQUEST_TIMEOUT_MS });
	return knownContextWindow(model);
}

/**
 * Whether the admin page lists the model: GPT-5.6 and newer, OpenAI's current generations in
 * September 2026, and of those only the ones that chat (not audio, pictures or search). Older
 * ones can still be typed.
 */
export function isListedModel(model: string): boolean {
	const version = gptVersion(model);
	if (!version) return false;
	const { major, minor, suffix } = version;
	if (major < 5 || (major === 5 && minor < 6)) return false;
	return !/audio|realtime|transcribe|tts|live|image|search|deep-research|chat/.test(suffix);
}

const SNAPSHOT_DATE = /-\d{4}-\d{2}-\d{2}$/;

/**
 * The models the admin page lists (isListedModel) that the key can use, the newest first,
 * without the dated snapshots of models also listed without a date (they can still be typed).
 * All of them when that leaves none: a compatible server behind OPENAI_BASE_URL has names of
 * its own.
 */
export async function listModels(): Promise<ModelChoice[]> {
	const client = await getClient();
	const all: OpenAI.Models.Model[] = [];
	for await (const model of client.models.list({ timeout: REQUEST_TIMEOUT_MS })) all.push(model);
	all.sort((a, b) => b.created - a.created);
	const ids = new Set(all.map((m) => m.id));
	const isSnapshot = (id: string) =>
		SNAPSHOT_DATE.test(id) && ids.has(id.replace(SNAPSHOT_DATE, ''));
	const listed = all.filter((m) => isListedModel(m.id) && !isSnapshot(m.id));
	return (listed.length ? listed : all).map((m) => ({
		id: m.id,
		name: null,
		description: null,
		contextWindow: knownContextWindow(m.id)
	}));
}

export function describeApiError(err: unknown): string {
	if (err instanceof MissingApiKeyError) return err.message;
	if (isSdkError(err, 'AuthenticationError')) {
		return `OpenAI didn't accept the API key. ${apiKeyHelp('openai')}`;
	}
	if (isSdkError(err, 'RateLimitError') && err.code !== 'insufficient_quota') {
		return 'Rate limited by OpenAI. Try again shortly.';
	}
	if (isSdkError(err, 'NotFoundError')) return `Model not found: ${shortApiError(err)}`;
	if (isSdkError(err, 'APIConnectionTimeoutError')) return "OpenAI didn't answer in time.";
	if (isSdkError(err, 'APIConnectionError')) {
		// fetch says "fetch failed"; the reason (ECONNREFUSED, ENOTFOUND...) is in its cause.
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach OpenAI (${why}).`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `OpenAI API error ${err.status}: ${shortApiError(err)}`;
	}
	return `OpenAI: ${shortApiError(err)}`;
}

/**
 * A failed request's status, code and message, for chatgpt-plan.ts to say in its words. Null for
 * what isn't a failure the API answered with (aborts, connection problems, other errors).
 */
export function failureOf(
	err: unknown
): { status: number | null; code: string | null; param: string | null; message: string } | null {
	if (err instanceof ReplyError) {
		return { status: null, code: err.code, param: null, message: err.message };
	}
	if (
		!isSdkError(err, 'APIError') ||
		isSdkError(err, 'APIUserAbortError') ||
		isSdkError(err, 'APIConnectionError')
	) {
		return null;
	}
	return {
		status: err.status ?? null,
		code: typeof err.code === 'string' ? err.code : null,
		param: typeof err.param === 'string' ? err.param : null,
		message: shortApiError(err)
	};
}

/** OpenAI's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		const body = err.error as { message?: unknown } | undefined;
		if (typeof body?.message === 'string') return body.message;
	}
	return err instanceof Error ? err.message : String(err);
}

export function isAbortError(err: unknown): boolean {
	return isSdkError(err, 'APIUserAbortError');
}
