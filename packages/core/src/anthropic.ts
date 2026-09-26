import { createHash } from 'node:crypto';
import Anthropic, { toFile } from '@anthropic-ai/sdk';
import { readConfig } from './config.ts';
import { RUN_COMMAND_TOOL } from './run-command.ts';

export const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'] as const;
export type Effort = (typeof EFFORTS)[number];

const CACHE_1H = { type: 'ephemeral', ttl: '1h' } as const;

let cached: { key: string | undefined; client: Anthropic } | undefined;

export class MissingApiKeyError extends Error {
	constructor() {
		super('No Anthropic API key. Set it with `btw key set anthropic`.');
	}
}

function apiKey(): string {
	const key = readConfig().anthropicApiKey || process.env.ANTHROPIC_API_KEY;
	if (!key) throw new MissingApiKeyError();
	return key;
}

export function getClient(): Anthropic {
	const key = apiKey();
	if (!cached || cached.key !== key) cached = { key, client: new Anthropic({ apiKey: key }) };
	return cached.client;
}

/** Haiku 4.5 predates adaptive thinking and effort. */
function supportsAdaptiveThinking(model: string): boolean {
	return !model.startsWith('claude-haiku-');
}

export type StreamEvent =
	| { type: 'block_start'; index: number; block: Anthropic.ContentBlock }
	| { type: 'delta'; index: number; text: string };

/**
 * One model call. The request shape must stay identical across calls in a conversation (only
 * `messages` grows), otherwise the prompt cache is lost.
 */
export async function streamTurn(opts: {
	model: string;
	effort: Effort;
	system: string;
	messages: Anthropic.MessageParam[];
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<Anthropic.Message> {
	const adaptive = supportsAdaptiveThinking(opts.model);
	const stream = getClient().messages.stream(
		{
			model: opts.model,
			max_tokens: 64000,
			// Automatic breakpoint on the growing tail, plus an explicit one on the frozen system
			// prompt. Both 1h: longer-TTL entries must come before shorter ones.
			cache_control: CACHE_1H,
			system: [{ type: 'text', text: opts.system, cache_control: CACHE_1H }],
			tools: [RUN_COMMAND_TOOL],
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
			opts.onEvent({ type: 'block_start', index: event.index, block: event.content_block });
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
		const file = await toFile(data, filesApiName(name), { type: mime });
		return (await getClient().files.upload({ file })).id;
	},
	/** False once the file was deleted (in the Console, say); other failures throw. */
	async exists(fileId: string): Promise<boolean> {
		try {
			await getClient().files.retrieveMetadata(fileId);
			return true;
		} catch (err) {
			if (err instanceof Anthropic.NotFoundError) return false;
			throw err;
		}
	},
	/** Resolves when the file is gone, also when it already was. */
	async remove(fileId: string): Promise<void> {
		try {
			await getClient().files.delete(fileId);
		} catch (err) {
			if (!(err instanceof Anthropic.NotFoundError)) throw err;
		}
	}
};

/**
 * What an uploaded PDF costs in every request of a conversation on this model. The API reads
 * the whole document, so it also throws for PDFs it can't use (encrypted, too many pages).
 */
export async function countDocumentTokens(model: string, fileId: string): Promise<number> {
	const count = await getClient().messages.countTokens({
		model,
		messages: [
			{ role: 'user', content: [{ type: 'document', source: { type: 'file', file_id: fileId } }] }
		]
	});
	return count.input_tokens;
}

export async function fetchContextWindow(model: string): Promise<number | null> {
	const info = await getClient().models.retrieve(model);
	return info.max_input_tokens ?? null;
}

export function describeApiError(err: unknown): string {
	if (err instanceof MissingApiKeyError) return err.message;
	if (err instanceof Anthropic.AuthenticationError) {
		return 'The Anthropic API key is missing or invalid. Set it with `btw key set anthropic`.';
	}
	if (err instanceof Anthropic.RateLimitError)
		return 'Rate limited by Anthropic. Try again shortly.';
	if (err instanceof Anthropic.NotFoundError) return `Model not found: ${err.message}`;
	if (err instanceof Anthropic.APIError) return `Anthropic API error ${err.status}: ${err.message}`;
	return err instanceof Error ? err.message : String(err);
}

/** The API's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (err instanceof Anthropic.APIError) {
		const body = err.error as { error?: { message?: unknown } } | undefined;
		if (typeof body?.error?.message === 'string') return body.error.message;
	}
	return err instanceof Error ? err.message : String(err);
}

export function isAbortError(err: unknown): boolean {
	return err instanceof Anthropic.APIUserAbortError;
}
