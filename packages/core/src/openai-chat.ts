import { createHash } from 'node:crypto';
import { setTimeout as sleep } from 'node:timers/promises';
import type Anthropic from '@anthropic-ai/sdk';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { Usage } from './conversations.ts';
import type { Effort, StreamEvent } from './models.ts';
import { openaiBaseUrl } from './openai.ts';

/*
 * Chats on OpenAI's models, through the Responses API, and its Files API for pictures and PDFs.
 * No SDK, like the Image API in openai.ts: a few endpoints and a stream of server-sent events,
 * and a plain fetch keeps the bundled CLI small. The rest of btw calls it through models.ts.
 *
 * Requests are stateless (`store: false`): like Anthropic's, every call sends the whole
 * transcript, so nothing depends on OpenAI keeping a conversation. A reply is stored as the
 * output items the API returned, its reasoning included in encrypted form, and sent back as it
 * came. btw's own blocks (people's messages, command results) are in Anthropic's format and are
 * turned into input items here, the same way on every call, so the request prefix stays
 * byte-identical and OpenAI's automatic prompt cache keeps serving it.
 */

/** An item of a response's `output`, as stored. */
export type OutputItem = { type: string } & Record<string, unknown>;

export interface OpenAIResponse {
	status?: string;
	output?: OutputItem[];
	incomplete_details?: { reason?: string } | null;
	usage?: OpenAIUsage | null;
}

interface OpenAIUsage {
	input_tokens?: number;
	input_tokens_details?: { cached_tokens?: number; cache_write_tokens?: number } | null;
	output_tokens?: number;
}

/** Retries after a dropped connection, an overload or a rate limit, like Anthropic's SDK. */
const RETRIES = 2;
const UPLOAD_TIMEOUT_MS = 5 * 60_000;
const REQUEST_TIMEOUT_MS = 60_000;

export class OpenAIChatError extends Error {
	/** The HTTP status, or null when OpenAI couldn't be reached or the stream failed. */
	readonly status: number | null;
	readonly code: string | null;
	constructor(message: string, status: number | null = null, code: string | null = null) {
		super(message);
		this.status = status;
		this.code = code;
	}
}

class MissingApiKeyError extends Error {
	constructor() {
		super(`No OpenAI API key. ${apiKeyHelp('openai')}`);
	}
}

export function isOpenAIError(err: unknown): boolean {
	return err instanceof OpenAIChatError || err instanceof MissingApiKeyError;
}

function apiKey(): string {
	const found = configuredApiKey('openai');
	if (!found) throw new MissingApiKeyError();
	return found.key;
}

/** Files and cached prompts belong to the key's project. A hash, so the key itself isn't stored. */
function accountOf(key: string): string {
	return createHash('sha256').update(key).digest('hex').slice(0, 16);
}

function retryable(status: number, code: string | null): boolean {
	if (status === 429) return code !== 'insufficient_quota';
	return status === 408 || status === 409 || status >= 500;
}

/** OpenAI says how long to wait in `retry-after-ms`, or `retry-after` in seconds. */
function retryDelay(attempt: number, res?: Response): number {
	const ms = Number(res?.headers.get('retry-after-ms'));
	if (ms > 0) return Math.min(ms, 30_000);
	const seconds = Number(res?.headers.get('retry-after'));
	if (seconds > 0) return Math.min(seconds, 30) * 1000;
	return 500 * 2 ** attempt + Math.random() * 250;
}

async function failure(res: Response): Promise<OpenAIChatError> {
	let error: { message?: unknown; code?: unknown } | undefined;
	try {
		error = ((await res.json()) as { error?: typeof error }).error;
	} catch {
		// not JSON
	}
	const message = typeof error?.message === 'string' && error.message.trim();
	const code = typeof error?.code === 'string' ? error.code : null;
	return new OpenAIChatError(message || res.statusText || 'no details', res.status, code);
}

/** One request to the API, retried when that's worth it. Throws an OpenAIChatError unless it's ok. */
async function call(
	path: string,
	options: {
		method?: 'GET' | 'POST' | 'DELETE';
		json?: unknown;
		form?: FormData;
		signal?: AbortSignal;
		timeoutMs?: number;
	} = {}
): Promise<Response> {
	const headers: Record<string, string> = { authorization: `Bearer ${apiKey()}` };
	if (options.json !== undefined) headers['content-type'] = 'application/json';
	const body = options.form ?? (options.json !== undefined ? JSON.stringify(options.json) : null);
	for (let attempt = 0; ; attempt++) {
		const signals = [options.signal, options.timeoutMs && AbortSignal.timeout(options.timeoutMs)];
		const signal = AbortSignal.any(signals.filter((s): s is AbortSignal => !!s));
		let res: Response;
		try {
			res = await fetch(`${openaiBaseUrl()}${path}`, {
				method: options.method ?? 'GET',
				headers,
				body,
				signal
			});
		} catch (err) {
			const e = err as { name?: string; cause?: { code?: string; message?: string } };
			if (options.signal?.aborted) throw err;
			if (e.name === 'TimeoutError') throw new OpenAIChatError("OpenAI didn't answer in time.");
			if (attempt < RETRIES) {
				await sleep(retryDelay(attempt), undefined, { signal: options.signal });
				continue;
			}
			const why = e.cause?.code ?? e.cause?.message ?? (err as Error).message;
			throw new OpenAIChatError(`Couldn't reach OpenAI (${why}).`);
		}
		if (res.ok) return res;
		const error = await failure(res);
		if (attempt < RETRIES && retryable(res.status, error.code)) {
			await sleep(retryDelay(attempt, res), undefined, { signal: options.signal });
			continue;
		}
		throw error;
	}
}

// --- turning the transcript into input items ---

type Block = { type?: unknown } & Record<string, unknown>;

/** What OpenAI returned in a reply, which goes back as it is. */
const OUTPUT_TYPES = new Set(['message', 'reasoning', 'function_call']);

/** A picture, PDF or text block of btw's (Anthropic's format) as an input content part. */
function inputPart(block: Block): Record<string, unknown> | null {
	const source = block.source as Record<string, unknown> | undefined;
	if (block.type === 'text') return { type: 'input_text', text: block.text };
	if (block.type === 'image' && source) {
		if (source.type === 'file')
			return { type: 'input_image', file_id: source.file_id, detail: 'auto' };
		if (source.type === 'base64') {
			const url = `data:${source.media_type};base64,${source.data}`;
			return { type: 'input_image', image_url: url, detail: 'auto' };
		}
		if (source.type === 'url')
			return { type: 'input_image', image_url: source.url, detail: 'auto' };
	}
	if (block.type === 'document' && source) {
		if (source.type === 'file') return { type: 'input_file', file_id: source.file_id };
		if (source.type === 'base64') {
			const filename = typeof block.title === 'string' ? block.title : 'document.pdf';
			return {
				type: 'input_file',
				filename,
				file_data: `data:${source.media_type};base64,${source.data}`
			};
		}
	}
	return null;
}

function toolOutput(content: unknown): unknown {
	if (!Array.isArray(content)) return typeof content === 'string' ? content : '';
	return (content as Block[]).flatMap((b) => inputPart(b) ?? []);
}

/**
 * The transcript as the Responses API's `input`. Messages from people and command results are
 * btw's own blocks, in Anthropic's format; replies are OpenAI's own output items, sent back as
 * they came, except reasoning without its encrypted content, which can't be read back without
 * `store`. A reply btw wrote itself (a notification continued in a chat) is plain text.
 */
export function toResponsesInput(messages: Anthropic.MessageParam[]): unknown[] {
	const input: unknown[] = [];
	for (const m of messages) {
		const blocks: Block[] =
			typeof m.content === 'string'
				? [{ type: 'text', text: m.content }]
				: (m.content as unknown as Block[]);
		if (m.role === 'assistant') {
			for (const b of blocks) {
				if (b.type === 'text' && typeof b.text === 'string' && b.text) {
					input.push({ role: 'assistant', content: b.text });
				} else if (b.type === 'reasoning' && !b.encrypted_content) {
					continue;
				} else if (typeof b.type === 'string' && OUTPUT_TYPES.has(b.type)) {
					input.push(b);
				}
			}
			continue;
		}
		// Results go on their own; what's around them stays in order, as user messages.
		let parts: unknown[] = [];
		const flush = () => {
			if (parts.length) input.push({ role: 'user', content: parts });
			parts = [];
		};
		for (const b of blocks) {
			if (b.type === 'tool_result') {
				flush();
				input.push({
					type: 'function_call_output',
					call_id: b.tool_use_id,
					output: toolOutput(b.content)
				});
			} else {
				const part = inputPart(b);
				if (part) parts.push(part);
			}
		}
		flush();
	}
	return input;
}

/** A tool as btw saves it (Anthropic's format) as a function tool. Not strict: `cwd` is optional. */
function functionTool(tool: Anthropic.Tool): Record<string, unknown> {
	return {
		type: 'function',
		name: tool.name,
		description: tool.description,
		parameters: tool.input_schema,
		strict: false
	};
}

/** GPT-4 models, and the chat-tuned ones ChatGPT uses, take no reasoning settings. */
export function supportsReasoning(model: string): boolean {
	return !/^(gpt-3|gpt-4|chatgpt-)/.test(model) && !/-chat/.test(model);
}

// --- a turn ---

/**
 * Accounts that can't have reasoning summaries (they need a verified organization), learned
 * from the first refusal. Their replies still reason; the chat just can't show it.
 */
const noSummaries = new Set<string>();

function refusesSummaries(err: unknown): boolean {
	return (
		err instanceof OpenAIChatError &&
		err.status === 400 &&
		/summar/i.test(err.message) &&
		/verif/i.test(err.message)
	);
}

function str(value: unknown): string {
	return typeof value === 'string' ? value : '';
}

function num(value: unknown): number {
	return typeof value === 'number' ? value : 0;
}

/** The events of a server-sent event stream, parsed from their `data:` lines. */
async function* serverSentEvents(
	body: ReadableStream<Uint8Array>
): AsyncGenerator<Record<string, unknown>> {
	const reader = body.getReader();
	const decoder = new TextDecoder();
	let buffer = '';
	try {
		for (;;) {
			const { done, value } = await reader.read();
			buffer += done ? decoder.decode() : decoder.decode(value, { stream: true });
			const events = buffer.split(/\r?\n\r?\n/);
			buffer = done ? '' : (events.pop() ?? '');
			for (const event of events) {
				const data = event
					.split(/\r?\n/)
					.filter((line) => line.startsWith('data:'))
					.map((line) => line.slice(5).replace(/^ /, ''))
					.join('\n');
				if (data && data !== '[DONE]') yield JSON.parse(data) as Record<string, unknown>;
			}
			if (done) return;
		}
	} finally {
		// The response is complete (or failed) before the stream ends: don't leave it open.
		reader.cancel().catch(() => {});
	}
}

/** Reads the stream, telling `onEvent` about each block as it grows, and returns the response. */
async function readStream(
	res: Response,
	onEvent: (event: StreamEvent) => void
): Promise<OpenAIResponse> {
	if (!res.body) throw new OpenAIChatError('OpenAI answered without a reply.');
	for await (const event of serverSentEvents(res.body)) {
		const index = num(event.output_index);
		switch (event.type) {
			case 'response.output_item.added': {
				const item = event.item as OutputItem;
				if (item.type === 'message')
					onEvent({ type: 'block_start', index, block: { type: 'text' } });
				else if (item.type === 'reasoning') {
					onEvent({ type: 'block_start', index, block: { type: 'thinking' } });
				} else if (item.type === 'function_call') {
					onEvent({ type: 'block_start', index, block: { type: 'tool', id: str(item.call_id) } });
				}
				break;
			}
			case 'response.output_text.delta':
			case 'response.refusal.delta':
			case 'response.reasoning_summary_text.delta':
				onEvent({ type: 'delta', index, text: str(event.delta) });
				break;
			case 'response.reasoning_summary_part.added':
				// Parts are paragraphs, as content-blocks.ts joins them.
				if (num(event.summary_index) > 0) onEvent({ type: 'delta', index, text: '\n\n' });
				break;
			case 'response.completed':
			case 'response.incomplete':
				return event.response as OpenAIResponse;
			case 'response.failed': {
				const error = (event.response as { error?: { message?: string; code?: string } } | null)
					?.error;
				throw new OpenAIChatError(error?.message || 'The reply failed.', null, error?.code ?? null);
			}
			case 'error':
				throw new OpenAIChatError(
					str(event.message) || 'The reply failed.',
					null,
					str(event.code) || null
				);
		}
	}
	throw new OpenAIChatError('The reply ended before it was complete.');
}

/** One model call, streamed. See models.ts for what stays fixed between calls. */
export async function streamResponse(opts: {
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	messages: Anthropic.MessageParam[];
	cacheKey: string;
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<OpenAIResponse> {
	const account = accountOf(apiKey());
	const request = (summaries: boolean) => ({
		model: opts.model,
		instructions: opts.system,
		input: toResponsesInput(opts.messages),
		tools: opts.tools.map(functionTool),
		store: false,
		stream: true,
		...(supportsReasoning(opts.model)
			? {
					reasoning: { effort: opts.effort, ...(summaries ? { summary: 'auto' } : {}) },
					include: ['reasoning.encrypted_content']
				}
			: {}),
		prompt_cache_key: opts.cacheKey
	});
	let res: Response;
	try {
		res = await call('/responses', {
			method: 'POST',
			json: request(!noSummaries.has(account)),
			signal: opts.signal
		});
	} catch (err) {
		if (noSummaries.has(account) || !refusesSummaries(err)) throw err;
		noSummaries.add(account);
		res = await call('/responses', { method: 'POST', json: request(false), signal: opts.signal });
	}
	return readStream(res, opts.onEvent);
}

/** One short exchange, not streamed, at low effort (see models.ts). */
export async function createResponse(opts: {
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<OpenAIResponse> {
	const res = await call('/responses', {
		method: 'POST',
		json: {
			model: opts.model,
			instructions: opts.system,
			input: opts.input,
			max_output_tokens: opts.maxTokens,
			store: false,
			...(supportsReasoning(opts.model) ? { reasoning: { effort: 'low' } } : {})
		},
		timeoutMs: opts.timeoutMs
	});
	return (await res.json()) as OpenAIResponse;
}

/** In Anthropic's words, as btw stores it: see ModelReply in models.ts. */
export function stopReason(response: OpenAIResponse): string {
	if (response.status === 'incomplete') {
		return response.incomplete_details?.reason === 'content_filter' ? 'refusal' : 'max_tokens';
	}
	const output = response.output ?? [];
	if (output.some((item) => item.type === 'function_call')) return 'tool_use';
	const refused = output.some(
		(item) =>
			item.type === 'message' &&
			Array.isArray(item.content) &&
			(item.content as Block[]).some((part) => part.type === 'refusal')
	);
	return refused ? 'refusal' : 'end_turn';
}

/** OpenAI counts cached tokens inside `input_tokens`; btw counts them apart, as Anthropic does. */
export function summarizeUsage(usage: OpenAIUsage | null | undefined): Usage {
	const cacheRead = num(usage?.input_tokens_details?.cached_tokens);
	const cacheWrite = num(usage?.input_tokens_details?.cache_write_tokens);
	return {
		input: Math.max(0, num(usage?.input_tokens) - cacheRead - cacheWrite),
		cacheRead,
		cacheWrite,
		output: num(usage?.output_tokens)
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
		const form = new FormData();
		// Pictures are uploaded for vision, PDFs as the model's input.
		form.append('purpose', mime.startsWith('image/') ? 'vision' : 'user_data');
		form.append('file', new Blob([new Uint8Array(data)], { type: mime }), uploadName(name));
		const res = await call('/files', { method: 'POST', form, timeoutMs: UPLOAD_TIMEOUT_MS });
		return ((await res.json()) as { id: string }).id;
	},
	/** False once the file was deleted; other failures throw. */
	async exists(fileId: string): Promise<boolean> {
		try {
			await call(`/files/${encodeURIComponent(fileId)}`, { timeoutMs: REQUEST_TIMEOUT_MS });
			return true;
		} catch (err) {
			if (err instanceof OpenAIChatError && err.status === 404) return false;
			throw err;
		}
	},
	/** Resolves when the file is gone, also when it already was. */
	async remove(fileId: string): Promise<void> {
		try {
			await call(`/files/${encodeURIComponent(fileId)}`, {
				method: 'DELETE',
				timeoutMs: REQUEST_TIMEOUT_MS
			});
		} catch (err) {
			if (!(err instanceof OpenAIChatError && err.status === 404)) throw err;
		}
	}
};

export async function countDocumentTokens(model: string, fileId: string): Promise<number> {
	const res = await call('/responses/input_tokens', {
		method: 'POST',
		json: { model, input: [{ role: 'user', content: [{ type: 'input_file', file_id: fileId }] }] },
		timeoutMs: REQUEST_TIMEOUT_MS
	});
	return num(((await res.json()) as { input_tokens?: unknown }).input_tokens);
}

/** Checks that the model exists. OpenAI's models API doesn't say how large its window is. */
export async function fetchContextWindow(model: string): Promise<number | null> {
	await call(`/models/${encodeURIComponent(model)}`, { timeoutMs: REQUEST_TIMEOUT_MS });
	return null;
}

export function describeApiError(err: unknown): string {
	if (err instanceof MissingApiKeyError) return err.message;
	if (!(err instanceof OpenAIChatError)) return err instanceof Error ? err.message : String(err);
	if (err.status === 401) return `OpenAI didn't accept the API key. ${apiKeyHelp('openai')}`;
	if (err.status === 429 && err.code !== 'insufficient_quota') {
		return 'Rate limited by OpenAI. Try again shortly.';
	}
	if (err.status === 404) return `Model not found: ${err.message}`;
	if (err.status === null) return `OpenAI: ${err.message}`;
	return `OpenAI API error ${err.status}: ${err.message}`;
}

/** OpenAI's own message, for notes shown to the model. */
export function shortApiError(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}
