import { randomUUID } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { Usage } from './conversations.ts';
import type { CacheTtl, Effort, StreamEvent } from './models.ts';

/*
 * Chats on the models OpenRouter serves (Anthropic's, OpenAI's, Google's, DeepSeek's...), through
 * its Chat Completions API. OpenAI's SDK speaks it: pointed at OpenRouter, it brings the same
 * retries and server-sent events as for OpenAI. The rest of btw calls it through models.ts.
 *
 * Every call sends the whole transcript, as with the other providers. A reply is stored as the
 * pieces the API returned: its `reasoning_details` as they came (Claude's signatures and
 * encrypted reasoning included, which must go back unchanged), its text as a `text` block, and
 * its `tool_calls`. btw's own blocks (Anthropic's format) become chat messages the same way on
 * every call, so the prefix stays byte-identical for the prompt cache. There's no Files API:
 * pictures and PDFs go inline, and only to models that take them (`modelInputs`).
 */

const REQUEST_TIMEOUT_MS = 60_000;
/** The model list is the same for everyone; it's asked for again after this long. */
const MODELS_TTL_MS = 60 * 60_000;

/** OPENROUTER_BASE_URL points it at a proxy or a stand-in server. */
export function openrouterBaseUrl(): string {
	return (process.env.OPENROUTER_BASE_URL || 'https://openrouter.ai/api/v1').replace(/\/+$/, '');
}

type Sdk = typeof import('openai');

/**
 * OpenAI's SDK, loaded on first use: the bundled CLI carries all of core, and most `btw`
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
	| 'AuthenticationError';

/** Whether `err` is one of the SDK's errors, which it can't be before the SDK was loaded. */
function isSdkError<K extends ErrorClass>(err: unknown, name: K): err is InstanceType<Sdk[K]> {
	return !!sdk && err instanceof sdk[name];
}

/** Something OpenRouter's side can't do, in words for people. */
class OpenRouterError extends Error {}

class MissingApiKeyError extends OpenRouterError {
	constructor() {
		super(`No OpenRouter API key. ${apiKeyHelp('openrouter')}`);
	}
}

/**
 * Errors from calls to OpenRouter. Its SDK errors are the same classes as OpenAI's, so
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

export function isOpenRouterError(err: unknown): boolean {
	return (
		err instanceof OpenRouterError || (err !== null && typeof err === 'object' && ours.has(err))
	);
}

function apiKey(): string {
	const found = configuredApiKey('openrouter');
	if (!found) throw new MissingApiKeyError();
	return found.key;
}

let cached: { key: string; baseURL: string; client: OpenAI } | undefined;

async function getClient(): Promise<OpenAI> {
	const key = apiKey();
	const baseURL = openrouterBaseUrl();
	const { OpenAI: Client } = await loadSdk();
	if (!cached || cached.key !== key || cached.baseURL !== baseURL) {
		// X-Title names btw on the account's activity page.
		const client = new Client({ apiKey: key, baseURL, defaultHeaders: { 'X-Title': 'btw' } });
		cached = { key, baseURL, client };
	}
	return cached.client;
}

// --- turning the transcript into chat messages ---

type Block = { type?: unknown } & Record<string, unknown>;
type CacheControl = { type: 'ephemeral'; ttl: CacheTtl };

type Part =
	| { type: 'text'; text: string }
	| { type: 'image_url'; image_url: { url: string } }
	| { type: 'file'; file: { filename: string; file_data: string } };

/** A piece of the model's reasoning, as OpenRouter returns it and wants it back. */
type ReasoningDetail = { type: string; index?: number } & Record<string, unknown>;

type ToolCallItem = { id: string; type: 'function'; function: { name: string; arguments: string } };

type ChatMessage =
	| { role: 'system'; content: string | (Part & { cache_control: CacheControl })[] }
	| { role: 'user'; content: Part[] }
	| {
			role: 'assistant';
			content: string | null;
			tool_calls?: ToolCallItem[];
			reasoning_details?: ReasoningDetail[];
	  }
	| { role: 'tool'; tool_call_id: string; content: string };

function isReasoningDetail(block: Block): block is ReasoningDetail {
	return typeof block.type === 'string' && block.type.startsWith('reasoning.');
}

function isToolCall(block: Block): block is ToolCallItem & Block {
	return block.type === 'function' && typeof block.id === 'string';
}

/** A picture, PDF or text block of btw's (Anthropic's format) as a content part. */
function inputPart(block: Block): Part | null {
	const source = block.source as Record<string, string> | undefined;
	if (block.type === 'text') return { type: 'text', text: String(block.text) };
	if (block.type === 'image' && source) {
		if (source.type === 'base64') {
			return {
				type: 'image_url',
				image_url: { url: `data:${source.media_type};base64,${source.data}` }
			};
		}
		if (source.type === 'url') return { type: 'image_url', image_url: { url: source.url } };
	}
	if (block.type === 'document' && source?.type === 'base64') {
		const filename = typeof block.title === 'string' ? block.title : 'document.pdf';
		return {
			type: 'file',
			file: { filename, file_data: `data:${source.media_type};base64,${source.data}` }
		};
	}
	return null;
}

/**
 * A command's result as a tool message's text, and the pictures in it (`btw view`), each after
 * the line naming it: a tool message takes only text, so they follow in a user message.
 */
function toolResult(content: unknown): { text: string; pictures: Part[] } {
	if (!Array.isArray(content))
		return { text: typeof content === 'string' ? content : '', pictures: [] };
	const blocks = content as Block[];
	const texts: string[] = [];
	const pictures: Part[] = [];
	blocks.forEach((b, i) => {
		if (b.type === 'text') texts.push(String(b.text));
		const part = b.type === 'image' ? inputPart(b) : null;
		if (!part) return;
		const label = blocks[i - 1];
		if (label?.type === 'text') pictures.push({ type: 'text', text: String(label.text) });
		pictures.push(part);
	});
	return { text: texts.join('\n'), pictures };
}

/**
 * The transcript as Chat Completions messages. Messages from people and command results are
 * btw's own blocks, in Anthropic's format; replies are what OpenRouter returned, put back into
 * one assistant message the way it came. A reply btw wrote itself (a notification continued in a
 * chat) is plain text. `cache`: Anthropic's models only cache what's marked.
 */
export function toChatMessages(
	system: string,
	messages: Anthropic.MessageParam[],
	cache: CacheControl | null = null
): ChatMessage[] {
	const out: ChatMessage[] = [
		{
			role: 'system',
			content: cache ? [{ type: 'text', text: system, cache_control: cache }] : system
		}
	];
	for (const m of messages) {
		const blocks: Block[] =
			typeof m.content === 'string'
				? [{ type: 'text', text: m.content }]
				: (m.content as unknown as Block[]);
		if (m.role === 'assistant') {
			const text = blocks.flatMap((b) => (b.type === 'text' ? [String(b.text)] : [])).join('');
			const calls = blocks.filter(isToolCall);
			const details = blocks.filter(isReasoningDetail);
			// Nothing to send for a reply that was only reasoning (cut off, say).
			if (!text && !calls.length) continue;
			out.push({
				role: 'assistant',
				content: text || null,
				...(calls.length ? { tool_calls: calls } : {}),
				...(details.length ? { reasoning_details: details } : {})
			});
			continue;
		}
		// Results come first, right after the reply that asked for them; the rest follows.
		const parts: Part[] = [];
		for (const b of blocks) {
			if (b.type === 'tool_result') {
				const result = toolResult(b.content);
				out.push({ role: 'tool', tool_call_id: String(b.tool_use_id), content: result.text });
				parts.push(...result.pictures);
			} else {
				const part = inputPart(b);
				if (part) parts.push(part);
			}
		}
		if (parts.length) out.push({ role: 'user', content: parts });
	}
	return out;
}

/** A tool as btw saves it (Anthropic's format) as a function tool. */
function functionTool(tool: Anthropic.Tool) {
	return {
		type: 'function' as const,
		function: { name: tool.name, description: tool.description, parameters: tool.input_schema }
	};
}

/** Claude, where OpenRouter serves it, only caches what's marked; the others cache on their own. */
export function marksCache(model: string): boolean {
	return /^~?anthropic\//.test(model);
}

// --- a turn ---

interface ChunkDelta {
	content?: string | null;
	refusal?: string | null;
	reasoning?: string | null;
	reasoning_details?: ReasoningDetail[] | null;
	tool_calls?:
		| {
				index?: number;
				id?: string | null;
				function?: { name?: string | null; arguments?: string | null } | null;
		  }[]
		| null;
}

type ChatUsage = {
	prompt_tokens?: number;
	completion_tokens?: number;
	prompt_tokens_details?: { cached_tokens?: number; cache_write_tokens?: number } | null;
} | null;

interface Chunk {
	choices?: { delta?: ChunkDelta | null; finish_reason?: string | null }[];
	usage?: ChatUsage;
}

export interface Reply {
	/** What's stored: reasoning details, then text, then tool calls. */
	content: unknown[];
	/** In Anthropic's words, as btw stores it: see ModelReply in models.ts. */
	stopReason: string;
	usage: Usage;
}

/** Fields of a reasoning detail that arrive in pieces; the others are the same in every piece. */
const PIECED = new Set(['text', 'summary', 'data', 'signature']);

/**
 * Reads the stream, telling `onEvent` about each block as it grows. Reasoning details and tool
 * calls arrive in pieces, keyed by their `index`, and are put together here.
 */
async function readStream(
	stream: AsyncIterable<Chunk>,
	onEvent: (event: StreamEvent) => void
): Promise<Reply> {
	const details = new Map<number, ReasoningDetail>();
	let lastDetail = -1;
	const calls = new Map<number, ToolCallItem>();
	let text = '';
	let refusal = '';
	let finish: string | null = null;
	let usage: ChatUsage = null;

	// The live blocks, numbered as they start.
	let blocks = 0;
	let thinkingBlock: number | null = null;
	let textBlock: number | null = null;
	const say = (kind: 'thinking' | 'text', delta: string) => {
		let index = kind === 'thinking' ? thinkingBlock : textBlock;
		if (index === null) {
			index = blocks++;
			if (kind === 'thinking') thinkingBlock = index;
			else textBlock = index;
			onEvent({ type: 'block_start', index, block: { type: kind } });
		}
		if (delta) onEvent({ type: 'delta', index, text: delta });
	};

	for await (const chunk of stream) {
		if (chunk.usage) usage = chunk.usage;
		const choice = chunk.choices?.[0];
		if (!choice) continue;
		if (choice.finish_reason) finish = choice.finish_reason;
		const delta = choice.delta ?? {};

		if (delta.reasoning_details?.length) {
			let shown = '';
			for (const piece of delta.reasoning_details) {
				// A piece without an index goes on with the detail before it, if it's of its type.
				const index =
					typeof piece.index === 'number'
						? piece.index
						: details.get(lastDetail)?.type === piece.type
							? lastDetail
							: details.size;
				lastDetail = index;
				const detail = details.get(index) ?? { type: piece.type };
				for (const [field, value] of Object.entries(piece)) {
					if (value === null || value === undefined) continue;
					if (PIECED.has(field) && typeof value === 'string') {
						detail[field] = `${typeof detail[field] === 'string' ? detail[field] : ''}${value}`;
					} else {
						detail[field] = value;
					}
				}
				details.set(index, detail);
				if (piece.type !== 'reasoning.encrypted') {
					shown +=
						typeof piece.text === 'string'
							? piece.text
							: typeof piece.summary === 'string'
								? piece.summary
								: '';
				}
			}
			say('thinking', shown);
		} else if (delta.reasoning) {
			say('thinking', delta.reasoning);
		}
		if (delta.content) {
			text += delta.content;
			say('text', delta.content);
		}
		if (delta.refusal) {
			refusal += delta.refusal;
			say('text', delta.refusal);
		}
		for (const piece of delta.tool_calls ?? []) {
			// A piece without an index goes on with the call before it, unless it starts a new one.
			const last = calls.get(calls.size - 1);
			const index =
				piece.index ?? (last && (!piece.id || piece.id === last.id) ? calls.size - 1 : calls.size);
			let call = calls.get(index);
			if (!call) {
				call = {
					id: piece.id || `call_${randomUUID().replace(/-/g, '').slice(0, 24)}`,
					type: 'function',
					function: { name: '', arguments: '' }
				};
				calls.set(index, call);
				onEvent({ type: 'block_start', index: blocks++, block: { type: 'tool', id: call.id } });
			}
			if (piece.function?.name) call.function.name = piece.function.name;
			if (piece.function?.arguments) call.function.arguments += piece.function.arguments;
		}
	}
	if (!finish) throw new OpenRouterError('OpenRouter: the reply ended before it was complete.');
	if (finish === 'error') throw new OpenRouterError('OpenRouter: the reply failed.');

	const said = [text, refusal].filter(Boolean).join('\n\n');
	// In the order the model made them, which is the order they must go back in.
	const content: unknown[] = [...details.entries()].sort(([a], [b]) => a - b).map(([, d]) => d);
	if (said) content.push({ type: 'text', text: said });
	content.push(...calls.values());
	return {
		content,
		stopReason: stopReason(finish, calls.size > 0, !!refusal),
		usage: summarizeUsage(usage)
	};
}

/** In Anthropic's words, as btw stores it: see ModelReply in models.ts. */
export function stopReason(finish: string | null, calls: boolean, refused = false): string {
	if (finish === 'length') return 'max_tokens';
	if (finish === 'content_filter' || refused) return 'refusal';
	return calls ? 'tool_use' : 'end_turn';
}

/**
 * One model call, streamed. See models.ts for what stays fixed between calls. `cacheKey` keeps a
 * conversation on the same provider behind OpenRouter, whose cache has its earlier calls.
 */
export function streamTurn(opts: {
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	cacheTtl: CacheTtl;
	cacheKey: string;
	messages: Anthropic.MessageParam[];
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<Reply> {
	return tagged(async () => {
		const client = await getClient();
		// Claude: the same marks as in anthropic.ts, on the system prompt and the growing tail.
		const cache = marksCache(opts.model)
			? ({ type: 'ephemeral', ttl: opts.cacheTtl } as const)
			: null;
		const body = {
			model: opts.model,
			messages: toChatMessages(opts.system, opts.messages, cache),
			tools: opts.tools.map(functionTool),
			reasoning: { effort: opts.effort },
			...(cache ? { cache_control: cache } : {}),
			session_id: opts.cacheKey,
			stream: true
		};
		const stream = await client.chat.completions.create(
			body as unknown as OpenAI.Chat.ChatCompletionCreateParamsStreaming,
			{ signal: opts.signal }
		);
		return readStream(stream as AsyncIterable<Chunk>, opts.onEvent);
	});
}

/** One short exchange, not streamed, at low effort (see models.ts). */
export function quickReply(opts: {
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<{ text: string | null; usage: Usage }> {
	return tagged(async () => {
		const client = await getClient();
		const body = {
			model: opts.model,
			messages: [
				{ role: 'system', content: opts.system },
				{ role: 'user', content: opts.input }
			],
			max_tokens: opts.maxTokens,
			reasoning: { effort: 'low' }
		};
		const reply = await client.chat.completions.create(
			body as unknown as OpenAI.Chat.ChatCompletionCreateParamsNonStreaming,
			{ timeout: opts.timeoutMs }
		);
		const choice = reply.choices?.[0];
		const usage = summarizeUsage(reply.usage as ChatUsage);
		if (choice?.finish_reason !== 'stop') return { text: null, usage };
		return { text: choice.message.content ?? '', usage };
	});
}

/** Like OpenAI, OpenRouter counts cached tokens inside `prompt_tokens`; btw counts them apart. */
export function summarizeUsage(usage: ChatUsage | undefined): Usage {
	const cacheRead = usage?.prompt_tokens_details?.cached_tokens ?? 0;
	const cacheWrite = usage?.prompt_tokens_details?.cache_write_tokens ?? 0;
	return {
		input: Math.max(0, (usage?.prompt_tokens ?? 0) - cacheRead - cacheWrite),
		cacheRead,
		cacheWrite,
		output: usage?.completion_tokens ?? 0
	};
}

// --- models, errors ---

interface ModelInfo {
	id: string;
	context_length?: number | null;
	top_provider?: { context_length?: number | null } | null;
	supported_parameters?: string[] | null;
	architecture?: { input_modalities?: string[] | null } | null;
}

let catalog: { at: number; models: Promise<Map<string, ModelInfo>> } | undefined;

/** Every model OpenRouter has, by id, kept for an hour. `fresh` asks again. */
function listModels(fresh = false): Promise<Map<string, ModelInfo>> {
	if (fresh || !catalog || Date.now() - catalog.at > MODELS_TTL_MS) {
		const models = tagged(async () => {
			const client = await getClient();
			const list = await client.get<{ data?: ModelInfo[] }>('/models', {
				timeout: REQUEST_TIMEOUT_MS
			});
			return new Map((list.data ?? []).map((m) => [m.id, m]));
		});
		const entry = { at: Date.now(), models };
		catalog = entry;
		// A failed list isn't kept.
		models.catch(() => {
			if (catalog === entry) catalog = undefined;
		});
	}
	return catalog!.models;
}

/** A variant (`:nitro`, `:online`...) is its model, routed differently, unless it's listed itself. */
async function modelInfo(model: string, fresh = false): Promise<ModelInfo | undefined> {
	const models = await listModels(fresh);
	return models.get(model) ?? models.get(model.replace(/:[^/]*$/, ''));
}

/** What the model can be sent besides text, as OpenRouter lists it. */
export async function modelInputs(model: string): Promise<{ pictures: boolean; pdfs: boolean }> {
	const inputs = (await modelInfo(model))?.architecture?.input_modalities ?? [];
	// A PDF goes only to models that read it themselves: for others, OpenRouter would run it
	// through a paid OCR service at every request, since each one carries the whole history.
	return { pictures: inputs.includes('image'), pdfs: inputs.includes('file') };
}

/**
 * Checks that OpenRouter has the model and that it can call tools, which btw needs to run
 * commands, and says how large its window is: the smaller of the model's and its main
 * provider's, since a window set too large would let a conversation outgrow the model for good.
 */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const info = await modelInfo(model, true);
	if (!info) {
		throw new OpenRouterError(
			`Model not found: OpenRouter has no model "${model}". Its ids look like anthropic/claude-sonnet-5 (see https://openrouter.ai/models).`
		);
	}
	if (!info.supported_parameters?.includes('tools')) {
		throw new OpenRouterError(
			`${model} can't call tools on OpenRouter, and btw needs them to run commands.`
		);
	}
	const windows = [info.context_length, info.top_provider?.context_length].filter(
		(n): n is number => typeof n === 'number' && n > 0
	);
	return windows.length ? Math.min(...windows) : null;
}

/** The HTTP status, or for an error in the middle of a stream, the code OpenRouter gives it. */
function statusOf(err: unknown): number | undefined {
	if (!isSdkError(err, 'APIError')) return undefined;
	const code = (err.error as { code?: unknown } | undefined)?.code;
	return err.status ?? (typeof code === 'number' ? code : undefined);
}

export function describeApiError(err: unknown): string {
	if (err instanceof OpenRouterError) return err.message;
	if (isSdkError(err, 'AuthenticationError')) {
		return `OpenRouter didn't accept the API key. ${apiKeyHelp('openrouter')}`;
	}
	if (statusOf(err) === 402) {
		return `OpenRouter needs more credits: ${shortApiError(err)} Add some at https://openrouter.ai/settings/credits.`;
	}
	if (statusOf(err) === 429) return `Rate limited by OpenRouter: ${shortApiError(err)}`;
	if (isSdkError(err, 'APIConnectionTimeoutError')) return "OpenRouter didn't answer in time.";
	if (isSdkError(err, 'APIConnectionError')) {
		// fetch says "fetch failed"; the reason (ECONNREFUSED, ENOTFOUND...) is in its cause.
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach OpenRouter (${why}).`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `OpenRouter API error ${err.status}: ${shortApiError(err)}`;
	}
	return `OpenRouter: ${shortApiError(err)}`;
}

/** What the provider behind OpenRouter said, when it passed on an error of theirs. */
function upstreamMessage(raw: unknown): string | null {
	let body = raw;
	if (typeof raw === 'string') {
		try {
			body = JSON.parse(raw);
		} catch {
			return raw.trim().slice(0, 300) || null;
		}
	}
	const found = body as { error?: { message?: unknown }; message?: unknown } | null;
	const message = found?.error?.message ?? found?.message;
	return typeof message === 'string' ? message : null;
}

/** OpenRouter's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		const body = err.error as
			{ message?: unknown; metadata?: { raw?: unknown; provider_name?: unknown } } | undefined;
		if (typeof body?.message === 'string') {
			const provider = body.metadata?.provider_name;
			const upstream = upstreamMessage(body.metadata?.raw);
			if (typeof provider !== 'string') return body.message;
			return `${body.message} (${provider}${upstream ? `: ${upstream}` : ''})`;
		}
	}
	return err instanceof Error ? err.message : String(err);
}

export function isAbortError(err: unknown): boolean {
	return isSdkError(err, 'APIUserAbortError');
}
