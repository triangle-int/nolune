import { createHash, randomUUID } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { Usage } from './conversations.ts';
import {
	heldElsewhere,
	heldElsewhereNote,
	placeholder,
	portableReply,
	unresolved,
	type Block,
	type ImageBlock,
	type Message,
	type PdfBlock,
	type TextBlock,
	type ToolResultBlock
} from './format.ts';
import type { CacheTtl, Effort, ModelChoice, StreamEvent } from './models.ts';

/*
 * Chats on the models OpenRouter serves (Anthropic's, OpenAI's, Google's, DeepSeek's...), through
 * its Chat Completions API. OpenAI's SDK speaks it: pointed at OpenRouter, it brings the same
 * retries and server-sent events as for OpenAI. The rest of btw calls it through models.ts.
 *
 * Every call sends the whole transcript, as with the other providers. A reply is stored as the
 * pieces the API returned: its `reasoning_details` as they came (Claude's signatures and
 * encrypted reasoning included, which must go back unchanged), its text as a `text` block, and
 * its `tool_calls`. btw's own blocks (Anthropic's format) become chat messages the same way on
 * every call, so the prefix stays byte-identical for the prompt cache. Pictures and PDFs go
 * through OpenRouter's Files API (in beta), and only to models that take them (`modelInputs`).
 */

const UPLOAD_TIMEOUT_MS = 5 * 60_000;
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
	| 'AuthenticationError'
	| 'NotFoundError';

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

// --- btw's format as OpenRouter takes it ---

type Stored = { type?: unknown } & Record<string, unknown>;
type CacheControl = { type: 'ephemeral'; ttl: CacheTtl };

type Part =
	| { type: 'text'; text: string }
	| { type: 'image_url'; image_url: { url: string } }
	| {
			type: 'file';
			file: { file_id: string; filename?: string } | { filename: string; file_data: string };
	  };

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

function isReasoningDetail(block: Stored): block is ReasoningDetail {
	return typeof block.type === 'string' && block.type.startsWith('reasoning.');
}

function toolCallItem(id: string, name: string, input: unknown): ToolCallItem {
	return { id, type: 'function', function: { name, arguments: JSON.stringify(input ?? {}) } };
}

/**
 * A text, picture or PDF block as a content part: an uploaded one as a `file` part with its id,
 * which Chat Completions takes for pictures too; one OpenRouter can't open, as a note.
 */
function inputPart(block: Block): Part | null {
	if (block.type === 'text') return { type: 'text', text: block.text };
	if (block.type !== 'image' && block.type !== 'pdf') return null;
	if (heldElsewhere(block, 'openrouter')) {
		return { type: 'text', text: heldElsewhereNote(block).text };
	}
	const { source } = block;
	if (source.type === 'media') unresolved(block);
	const filename = block.type === 'pdf' ? block.name || 'document.pdf' : null;
	if (source.type === 'uploaded') {
		const file = filename ? { file_id: source.fileId, filename } : { file_id: source.fileId };
		return { type: 'file', file };
	}
	const data = `data:${source.mime};base64,${source.data}`;
	if (!filename) return { type: 'image_url', image_url: { url: data } };
	return { type: 'file', file: { filename, file_data: data } };
}

/**
 * A command's result as a tool message's text, and the pictures and PDFs in it (`btw view`), each
 * after the line naming it: a tool message takes only text, so they follow in a user message.
 */
function toolResult(content: ToolResultBlock['content']): { text: string; files: Part[] } {
	if (typeof content === 'string') return { text: content, files: [] };
	const texts: string[] = [];
	const files: Part[] = [];
	content.forEach((b, i) => {
		if (b.type === 'text') texts.push(b.text);
		else if (b.type === 'other') texts.push(`[${placeholder(b)}]`);
		const part = b.type === 'image' || b.type === 'pdf' ? inputPart(b) : null;
		if (!part) return;
		const label = content[i - 1];
		if (label?.type === 'text') files.push({ type: 'text', text: label.text });
		files.push(part);
	});
	return { text: texts.join('\n'), files };
}

/**
 * A reply as one assistant message, from its text and calls; with `details`, the reasoning that
 * goes back to the model that wrote it. Null for one with nothing to send (only reasoning, cut
 * off, say).
 */
function assistantMessage(
	texts: string[],
	calls: ToolCallItem[],
	details: ReasoningDetail[] = []
): ChatMessage | null {
	const text = texts.filter(Boolean).join('\n\n');
	if (!text && !calls.length) return null;
	return {
		role: 'assistant',
		content: text || null,
		...(calls.length ? { tool_calls: calls } : {}),
		...(details.length ? { reasoning_details: details } : {})
	};
}

/** A reply of this model's, as OpenRouter returned it. */
function nativeMessage(content: unknown[], withReasoning: boolean): ChatMessage | null {
	const blocks = content as Stored[];
	const texts = blocks.flatMap((b) => (b.type === 'text' ? [String(b.text ?? '')] : []));
	const calls = blocks.flatMap((b): ToolCallItem[] => {
		if (b.type === 'function' && typeof b.id === 'string') return [b as unknown as ToolCallItem];
		// A row that didn't record who wrote it (tests).
		if (b.type === 'tool_use') return [toolCallItem(String(b.id), String(b.name), b.input)];
		return [];
	});
	return assistantMessage(texts, calls, withReasoning ? blocks.filter(isReasoningDetail) : []);
}

/**
 * A conversation's messages as Chat Completions messages for `model`. Replies it wrote go back as
 * they came, reasoning included, except from before the system prompt was built again: through
 * OpenRouter it may be Claude's thinking, which is bound to the prompt. Replies from another model
 * or provider (the conversation switched), and btw's own, go as their text and calls. `cache`:
 * Claude only caches what's marked.
 */
export function toChatMessages(
	system: string,
	messages: Message[],
	model: string,
	cache: CacheControl | null = null
): ChatMessage[] {
	const out: ChatMessage[] = [
		{
			role: 'system',
			content: cache ? [{ type: 'text', text: system, cache_control: cache }] : system
		}
	];
	for (const m of messages) {
		if (m.role === 'assistant') {
			const native = m.native;
			let reply: ChatMessage | null;
			if (
				native &&
				(native.provider === null || (native.provider === 'openrouter' && native.model === model))
			) {
				reply = nativeMessage(native.content, !m.beforePromptChange);
			} else {
				const portable = portableReply(m.blocks);
				reply = assistantMessage(
					portable.flatMap((b) => (b.type === 'text' ? [b.text] : [])),
					portable.flatMap((b) =>
						b.type === 'tool_call' ? [toolCallItem(b.id, b.name, b.input)] : []
					)
				);
			}
			if (reply) out.push(reply);
			continue;
		}
		// Results come first, right after the reply that asked for them; the rest follows.
		const parts: Part[] = [];
		for (const b of m.blocks) {
			if (b.type === 'tool_result') {
				const result = toolResult(b.content);
				out.push({ role: 'tool', tool_call_id: b.callId, content: result.text });
				parts.push(...result.files);
			} else {
				const part = inputPart(b);
				if (part) parts.push(part);
			}
		}
		if (parts.length) out.push({ role: 'user', content: parts });
	}
	return out;
}

/** What a model reads instead of a picture or PDF it can't take. */
function unreadableNote(block: ImageBlock | PdfBlock, model: string): TextBlock {
	const what =
		block.type === 'image'
			? `Picture not shown: ${model} can't see pictures`
			: `PDF not shown: ${model} doesn't read PDFs itself`;
	return { type: 'text', text: `[${what}. The line before this says where its file is.]` };
}

/**
 * The messages with each picture and PDF `model` can't take as a note, before resolveFiles
 * uploads them: a chat that switched to a text-only model may hold them, and a request carrying
 * one would fail. Messages without any are returned as they are.
 */
export async function readableMessages(messages: Message[], model: string): Promise<Message[]> {
	const isFile = (b: Block) => b.type === 'image' || b.type === 'pdf';
	const holdsFiles = messages.some((m) =>
		m.blocks.some(
			(b) =>
				isFile(b) ||
				(b.type === 'tool_result' && Array.isArray(b.content) && b.content.some(isFile))
		)
	);
	if (!holdsFiles) return messages;
	const inputs = await modelInputs(model);
	if (inputs.pictures && inputs.pdfs) return messages;
	return messages.map((m) => {
		let changed = false;
		const readable = <B extends Block>(b: B): B | TextBlock => {
			if ((b.type !== 'image' || inputs.pictures) && (b.type !== 'pdf' || inputs.pdfs)) return b;
			changed = true;
			return unreadableNote(b as unknown as ImageBlock | PdfBlock, model);
		};
		const blocks = m.blocks.map((b): Block =>
			b.type === 'tool_result' && Array.isArray(b.content)
				? { ...b, content: b.content.map(readable) }
				: readable(b)
		);
		return changed ? { ...m, blocks } : m;
	});
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
	/** In btw's format, with pictures and PDFs as OpenRouter gets them (resolveFiles). */
	messages: Message[];
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
			messages: toChatMessages(opts.system, opts.messages, opts.model, cache),
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

// --- files, models, errors ---

/** Files belong to the key's workspace. A hash, so the key itself isn't stored. */
function accountOf(key: string): string {
	return createHash('sha256').update(key).digest('hex').slice(0, 16);
}

/** Keeps names short and plain; the model never needs them, the Files API only stores them. */
function uploadName(name: string): string {
	// eslint-disable-next-line no-control-regex
	const clean = name.replace(/[<>:"|?*\\/\x00-\x1f]/g, '_').trim();
	return clean.slice(-200) || 'file';
}

/**
 * OpenRouter's Files API, as provider-files.ts uses it (see FileStore there). It answers OpenAI's
 * SDK in OpenAI's shape, missing files included, so this is openai-chat.ts's store on OpenRouter.
 * It tells the file's type from its content; `purpose` is only there because the SDK wants one.
 */
export const openrouterFiles = {
	account(): string {
		return accountOf(apiKey());
	},
	upload(data: Buffer, name: string, mime: string): Promise<string> {
		return tagged(async () => {
			const client = await getClient();
			const file = await (await loadSdk()).toFile(data, uploadName(name), { type: mime });
			const purpose = 'user_data' as const;
			return (await client.files.create({ file, purpose }, { timeout: UPLOAD_TIMEOUT_MS })).id;
		});
	},
	/** False once the file was deleted; other failures throw. */
	exists(fileId: string): Promise<boolean> {
		return tagged(async () => {
			const client = await getClient();
			try {
				await client.files.retrieve(fileId, { timeout: REQUEST_TIMEOUT_MS });
				return true;
			} catch (err) {
				if (isSdkError(err, 'NotFoundError')) return false;
				throw err;
			}
		});
	},
	/** Resolves when the file is gone, also when it already was. */
	remove(fileId: string): Promise<void> {
		return tagged(async () => {
			const client = await getClient();
			try {
				await client.files.delete(fileId, { timeout: REQUEST_TIMEOUT_MS });
			} catch (err) {
				if (!isSdkError(err, 'NotFoundError')) throw err;
			}
		});
	}
};

interface ModelInfo {
	id: string;
	name?: string | null;
	/** When OpenRouter added it, in seconds. */
	created?: number | null;
	context_length?: number | null;
	top_provider?: { context_length?: number | null } | null;
	supported_parameters?: string[] | null;
	architecture?: { input_modalities?: string[] | null } | null;
}

let catalog: { at: number; models: Promise<Map<string, ModelInfo>> } | undefined;

/** Every model OpenRouter has, by id, kept for an hour. `fresh` asks again. */
function catalogOf(fresh = false): Promise<Map<string, ModelInfo>> {
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
	const models = await catalogOf(fresh);
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
	if (!callsTools(info)) {
		throw new OpenRouterError(
			`${model} can't call tools on OpenRouter, and btw needs them to run commands.`
		);
	}
	return windowOf(info);
}

function callsTools(info: ModelInfo): boolean {
	return !!info.supported_parameters?.includes('tools');
}

function windowOf(info: ModelInfo): number | null {
	const windows = [info.context_length, info.top_provider?.context_length].filter(
		(n): n is number => typeof n === 'number' && n > 0
	);
	return windows.length ? Math.min(...windows) : null;
}

/**
 * The models a preset can take (they call tools), for the admin page, the newest first, with
 * OpenRouter's names and the window a preset gets. `:batch` variants are left out: they're for
 * OpenRouter's batch API, not chats.
 */
export async function listModels(): Promise<ModelChoice[]> {
	const models = [...(await catalogOf(true)).values()].filter(
		(m) => callsTools(m) && !m.id.endsWith(':batch')
	);
	models.sort((a, b) => (b.created ?? 0) - (a.created ?? 0));
	return models.map((m) => ({
		id: m.id,
		name: m.name ?? null,
		description: null,
		contextWindow: windowOf(m)
	}));
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
