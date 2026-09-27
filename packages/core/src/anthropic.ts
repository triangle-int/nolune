import { createHash } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { CacheTtl, Effort, ModelChoice, StreamEvent } from './models.ts';

/*
 * Chats on Claude, through Anthropic's Messages API and its SDK, and the Files API for pictures
 * and PDFs. The rest of btw calls it through models.ts.
 */

type Sdk = typeof import('@anthropic-ai/sdk');
type ErrorClass =
	'APIError' | 'APIUserAbortError' | 'AuthenticationError' | 'NotFoundError' | 'RateLimitError';

let sdk: Sdk | undefined;

/**
 * The SDK is imported on first use. The CLI bundles all of core, and most `btw` commands never
 * call Claude: importing it up front would make each of them slower.
 */
async function loadSdk(): Promise<Sdk> {
	sdk ??= await import('@anthropic-ai/sdk');
	return sdk;
}

/** Before the SDK is loaded no error can be one of its classes, so this doesn't load it. */
function isSdkError<K extends ErrorClass>(err: unknown, name: K): err is InstanceType<Sdk[K]> {
	return sdk !== undefined && err instanceof sdk[name];
}

let cached: { key: string | undefined; client: Anthropic } | undefined;

export class MissingApiKeyError extends Error {
	constructor() {
		super(`No Anthropic API key. ${apiKeyHelp('anthropic')}`);
	}
}

function apiKey(): string {
	const found = configuredApiKey('anthropic');
	if (!found) throw new MissingApiKeyError();
	return found.key;
}

export async function getClient(): Promise<Anthropic> {
	const key = apiKey();
	const { Anthropic: Client } = await loadSdk();
	if (!cached || cached.key !== key) cached = { key, client: new Client({ apiKey: key }) };
	return cached.client;
}

/** Haiku 4.5 predates adaptive thinking and effort. */
export function supportsAdaptiveThinking(model: string): boolean {
	return !model.startsWith('claude-haiku-');
}

/**
 * One model call. The request shape must stay identical across calls in a conversation (only
 * `messages` grows), otherwise the prompt cache is lost: `tools`, `system` and `cacheTtl` are the
 * conversation's own, fixed when it was created.
 */
export async function streamTurn(opts: {
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	cacheTtl: CacheTtl;
	messages: Anthropic.MessageParam[];
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<Anthropic.Message> {
	const adaptive = supportsAdaptiveThinking(opts.model);
	const cache = { type: 'ephemeral', ttl: opts.cacheTtl } as const;
	const client = await getClient();
	const stream = client.messages.stream(
		{
			model: opts.model,
			max_tokens: 64000,
			// Automatic breakpoint on the growing tail, plus an explicit one on the frozen system
			// prompt. The same TTL on both: longer-TTL entries must come before shorter ones.
			cache_control: cache,
			system: [{ type: 'text', text: opts.system, cache_control: cache }],
			tools: opts.tools,
			...(adaptive
				? {
						// "summarized" also returns the short notes newer models write between tool calls.
						thinking: { type: 'adaptive', display: 'summarized' },
						output_config: { effort: opts.effort }
					}
				: {}),
			messages: opts.messages
		},
		{ signal: opts.signal }
	);

	for await (const event of stream) {
		if (event.type === 'content_block_start') {
			const b = event.content_block;
			const block =
				b.type === 'text' || b.type === 'thinking'
					? { type: b.type }
					: b.type === 'tool_use'
						? { type: 'tool' as const, id: b.id }
						: null;
			if (block) opts.onEvent({ type: 'block_start', index: event.index, block });
		} else if (event.type === 'content_block_delta') {
			if (event.delta.type === 'text_delta') {
				opts.onEvent({ type: 'delta', index: event.index, text: event.delta.text });
			} else if (event.delta.type === 'thinking_delta') {
				opts.onEvent({ type: 'delta', index: event.index, text: event.delta.thinking });
			}
		}
	}
	return stream.finalMessage();
}

/** One short exchange, not streamed, at low effort where the model takes one (see models.ts). */
export async function createMessage(opts: {
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<Anthropic.Message> {
	const client = await getClient();
	return client.messages.create(
		{
			model: opts.model,
			max_tokens: opts.maxTokens,
			system: opts.system,
			...(supportsAdaptiveThinking(opts.model)
				? { output_config: { effort: 'low' as const } }
				: {}),
			messages: [{ role: 'user', content: opts.input }]
		},
		{ timeout: opts.timeoutMs }
	);
}

/** The Files API rejects names over 255 characters and with any of these in them. */
function filesApiName(name: string): string {
	// eslint-disable-next-line no-control-regex
	const clean = name.replace(/[<>:"|?*\\/\x00-\x1f]/g, '_').trim();
	return clean.slice(-200) || 'file';
}

/**
 * The Files API: pictures and PDFs are uploaded once and requests refer to them by id, so the
 * history doesn't resend their bytes with every step.
 */
export const anthropicFiles = {
	/** Files belong to the API key's workspace. A hash, so the key itself isn't stored. */
	account(): string {
		return createHash('sha256').update(apiKey()).digest('hex').slice(0, 16);
	},
	async upload(data: Buffer, name: string, mime: string): Promise<string> {
		const client = await getClient();
		const { toFile } = await loadSdk();
		const file = await toFile(data, filesApiName(name), { type: mime });
		return (await client.files.upload({ file })).id;
	},
	/** False once the file was deleted (in the Console, say); other failures throw. */
	async exists(fileId: string): Promise<boolean> {
		const client = await getClient();
		try {
			await client.files.retrieveMetadata(fileId);
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
			await client.files.delete(fileId);
		} catch (err) {
			if (!isSdkError(err, 'NotFoundError')) throw err;
		}
	}
};

/**
 * What an uploaded PDF costs in every request of a conversation on this model. The API reads
 * the whole document, so it also throws for PDFs it can't use (encrypted, too many pages).
 */
export async function countDocumentTokens(model: string, fileId: string): Promise<number> {
	const client = await getClient();
	const count = await client.messages.countTokens({
		model,
		messages: [
			{ role: 'user', content: [{ type: 'document', source: { type: 'file', file_id: fileId } }] }
		]
	});
	return count.input_tokens;
}

export async function fetchContextWindow(model: string): Promise<number | null> {
	const client = await getClient();
	const info = await client.models.retrieve(model);
	return info.max_input_tokens ?? null;
}

/** Every model the key can use, the newest first, as Anthropic lists them. */
export async function listModels(): Promise<ModelChoice[]> {
	const client = await getClient();
	const models: ModelChoice[] = [];
	for await (const info of client.models.list({ limit: 100 })) {
		models.push({
			id: info.id,
			name: info.display_name,
			description: null,
			contextWindow: info.max_input_tokens ?? null
		});
	}
	return models;
}

export function describeApiError(err: unknown): string {
	if (err instanceof MissingApiKeyError) return err.message;
	if (isSdkError(err, 'AuthenticationError')) {
		return `Anthropic didn't accept the API key. ${apiKeyHelp('anthropic')}`;
	}
	if (isSdkError(err, 'RateLimitError')) return 'Rate limited by Anthropic. Try again shortly.';
	if (isSdkError(err, 'NotFoundError')) return `Model not found: ${err.message}`;
	if (isSdkError(err, 'APIError')) return `Anthropic API error ${err.status}: ${err.message}`;
	return err instanceof Error ? err.message : String(err);
}

/** The API's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		const body = err.error as { error?: { message?: unknown } } | undefined;
		if (typeof body?.error?.message === 'string') return body.error.message;
	}
	return err instanceof Error ? err.message : String(err);
}

export function isAbortError(err: unknown): boolean {
	return isSdkError(err, 'APIUserAbortError');
}
