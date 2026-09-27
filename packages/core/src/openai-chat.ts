import { createHash } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import type { OpenAI } from 'openai';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { Usage } from './conversations.ts';
import type { Effort, StreamEvent } from './models.ts';
import { openaiBaseUrl } from './openai.ts';

/*
 * Chats on OpenAI's models, through the Responses API and OpenAI's SDK, and its Files API for
 * pictures and PDFs. The rest of btw calls it through models.ts.
 *
 * Requests are stateless (`store: false`): like Anthropic's, every call sends the whole
 * transcript, so nothing depends on OpenAI keeping a conversation. A reply is stored as the
 * output items the API returned, its reasoning included in encrypted form, and sent back as it
 * came. btw's own blocks (people's messages, command results) are in Anthropic's format and are
 * turned into input items here, the same way on every call, so the request prefix stays
 * byte-identical and OpenAI's automatic prompt cache keeps serving it.
 */

type InputItem = OpenAI.Responses.ResponseInputItem;
type InputPart = OpenAI.Responses.ResponseInputContent;
type OutputPart = OpenAI.Responses.ResponseFunctionCallOutputItem;

const UPLOAD_TIMEOUT_MS = 5 * 60_000;
const REQUEST_TIMEOUT_MS = 60_000;

type Sdk = typeof import('openai');

/**
 * The SDK, loaded on first use. The bundled CLI carries all of core, and loading the SDK when
 * it starts would slow down every `btw` command the agent runs, which almost never call OpenAI.
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
class ReplyError extends Error {}

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

// --- turning the transcript into input items ---

type Block = { type?: unknown } & Record<string, unknown>;

/** What OpenAI returned in a reply, which goes back as it is. */
const OUTPUT_TYPES = new Set(['message', 'reasoning', 'function_call']);

/** A picture, PDF or text block of btw's (Anthropic's format) as an input content part. */
function inputPart(block: Block): InputPart | null {
	const source = block.source as Record<string, string> | undefined;
	if (block.type === 'text') return { type: 'input_text', text: String(block.text) };
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
			const data = `data:${source.media_type};base64,${source.data}`;
			return { type: 'input_file', filename, file_data: data };
		}
	}
	return null;
}

function toolOutput(content: unknown): string | OutputPart[] {
	if (!Array.isArray(content)) return typeof content === 'string' ? content : '';
	return (content as Block[]).flatMap((b) => (inputPart(b) as OutputPart | null) ?? []);
}

/**
 * The transcript as the Responses API's `input`. Messages from people and command results are
 * btw's own blocks, in Anthropic's format; replies are OpenAI's own output items, sent back as
 * they came, except reasoning without its encrypted content, which can't be read back without
 * `store`. A reply btw wrote itself (a notification continued in a chat) is plain text.
 */
export function toResponsesInput(messages: Anthropic.MessageParam[]): InputItem[] {
	const input: InputItem[] = [];
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
					input.push(b as unknown as InputItem);
				}
			}
			continue;
		}
		// Results go on their own; what's around them stays in order, as user messages.
		let parts: InputPart[] = [];
		const flush = () => {
			if (parts.length) input.push({ role: 'user', content: parts });
			parts = [];
		};
		for (const b of blocks) {
			if (b.type === 'tool_result') {
				flush();
				input.push({
					type: 'function_call_output',
					call_id: String(b.tool_use_id),
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
function functionTool(tool: Anthropic.Tool): OpenAI.Responses.FunctionTool {
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

/** Reads the stream, telling `onEvent` about each block as it grows, and returns the response. */
async function readStream(
	stream: AsyncIterable<OpenAI.Responses.ResponseStreamEvent>,
	onEvent: (event: StreamEvent) => void
): Promise<OpenAI.Responses.Response> {
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
			case 'response.output_text.delta':
			case 'response.refusal.delta':
			case 'response.reasoning_summary_text.delta':
				onEvent({ type: 'delta', index: event.output_index, text: event.delta });
				break;
			case 'response.reasoning_summary_part.added':
				// Parts are paragraphs, as content-blocks.ts joins them.
				if (event.summary_index > 0) {
					onEvent({ type: 'delta', index: event.output_index, text: '\n\n' });
				}
				break;
			case 'response.completed':
			case 'response.incomplete':
				// Leaving the loop closes the stream.
				return event.response;
			case 'response.failed':
				throw new ReplyError(event.response.error?.message || 'The reply failed.');
		}
	}
	throw new ReplyError('The reply ended before it was complete.');
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
}): Promise<OpenAI.Responses.Response> {
	const client = await getClient();
	const account = accountOf(apiKey());
	const request = (summaries: boolean) =>
		client.responses.create(
			{
				model: opts.model,
				instructions: opts.system,
				input: toResponsesInput(opts.messages),
				tools: opts.tools.map(functionTool),
				store: false,
				stream: true,
				...(supportsReasoning(opts.model)
					? {
							reasoning: {
								effort: opts.effort,
								...(summaries ? { summary: 'auto' as const } : {})
							},
							include: ['reasoning.encrypted_content' as const]
						}
					: {}),
				prompt_cache_key: opts.cacheKey
			},
			{ signal: opts.signal }
		);
	let stream: AsyncIterable<OpenAI.Responses.ResponseStreamEvent>;
	try {
		stream = await request(!noSummaries.has(account));
	} catch (err) {
		if (noSummaries.has(account) || !refusesSummaries(err)) throw err;
		noSummaries.add(account);
		stream = await request(false);
	}
	return readStream(stream, opts.onEvent);
}

/** One short exchange, not streamed, at low effort (see models.ts). */
export async function createResponse(opts: {
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<OpenAI.Responses.Response> {
	const client = await getClient();
	return client.responses.create(
		{
			model: opts.model,
			instructions: opts.system,
			input: opts.input,
			max_output_tokens: opts.maxTokens,
			store: false,
			...(supportsReasoning(opts.model) ? { reasoning: { effort: 'low' as const } } : {})
		},
		{ timeout: opts.timeoutMs }
	);
}

/** In Anthropic's words, as btw stores it: see ModelReply in models.ts. */
export function stopReason(response: {
	status?: string;
	output?: readonly unknown[];
	incomplete_details?: { reason?: string } | null;
}): string {
	if (response.status === 'incomplete') {
		return response.incomplete_details?.reason === 'content_filter' ? 'refusal' : 'max_tokens';
	}
	const output = (response.output ?? []) as Block[];
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

/**
 * The model's context window when btw knows it: OpenAI's models API doesn't say. Flagships
 * since GPT-5.4 (gpt-5.4, gpt-5.5-pro, gpt-6-astra, and their dated snapshots) have 1,050,000
 * tokens. Other models (mini, nano, codex, older ones) may have much less, and a window set too
 * large would let a conversation grow past what the model takes, for good, so they get none.
 */
export function knownContextWindow(model: string): number | null {
	const match = /^gpt-(\d+)(?:\.(\d+))?(-.*)?$/.exec(model);
	if (!match) return null;
	const [major, minor] = [Number(match[1]), Number(match[2] ?? 0)];
	if (major < 5 || (major === 5 && minor < 4)) return null;
	// GPT-6's names, Pro, and a snapshot's date; any other suffix may be a smaller model.
	const flagship = /^(-(astra|sol|luna))?(-pro)?(-\d{4}-\d{2}-\d{2})?$/.test(match[3] ?? '');
	return flagship ? FLAGSHIP_CONTEXT_WINDOW : null;
}

/** Checks that the model exists, and says how large its window is when btw knows it. */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const client = await getClient();
	await client.models.retrieve(model, { timeout: REQUEST_TIMEOUT_MS });
	return knownContextWindow(model);
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
