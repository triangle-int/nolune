import type Anthropic from '@anthropic-ai/sdk';
import * as anthropic from './anthropic.ts';
import * as claudePlan from './claude-plan.ts';
import type { Usage } from './conversations.ts';
import { replyBlocks, toolCalls, type Message, type ToolCallBlock } from './format.ts';
import * as openai from './openai-chat.ts';

/*
 * A model call as the rest of btw sees it, whichever provider runs it. Each provider's module
 * speaks its own API; this one picks the module for a conversation's provider and turns what it
 * returns into the same shape. Requests are built from btw's own format (format.ts) by each
 * provider's module. The reply's `content` is still the provider's own, and is stored and sent
 * back exactly as it came.
 *
 * `claude-plan` is the exception: Claude Code runs its agent loop (claude-plan.ts), so the runner
 * hands it whole turns rather than calling streamTurn.
 */

export const PROVIDERS = ['anthropic', 'openai', 'claude-plan'] as const;
export type Provider = (typeof PROVIDERS)[number];

export function isProvider(value: string): value is Provider {
	return (PROVIDERS as readonly string[]).includes(value);
}

/** How the admin page and the CLI name each provider. */
export const PROVIDER_LABELS: Record<Provider, string> = {
	anthropic: 'Anthropic',
	openai: 'OpenAI',
	'claude-plan': 'Claude plan'
};

/** Providers whose chats run through Claude Code and a Claude plan rather than an API key. */
export function runsOnClaudeCode(provider: Provider): provider is 'claude-plan' {
	return provider === 'claude-plan';
}

export const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'] as const;
export type Effort = (typeof EFFORTS)[number];

/**
 * How long a cached prompt lives: an hour for chats people come back to, 5 minutes for
 * subagents. Only Anthropic takes it; OpenAI caches on its own.
 */
export type CacheTtl = '5m' | '1h';

/** The live reply, block by block, for the chat to draw while it streams. */
export type StreamEvent =
	| {
			type: 'block_start';
			index: number;
			block: { type: 'text' | 'thinking' | 'tool'; id?: string };
	  }
	| { type: 'delta'; index: number; text: string };

export type ToolCall = ToolCallBlock;

export interface ModelReply {
	/** What the provider returned, to store and send back unchanged. */
	content: unknown[];
	/**
	 * In Anthropic's words, whichever provider answered: `end_turn`, `tool_use` (the calls are
	 * complete and can run), `max_tokens` (cut off) or `refusal`.
	 */
	stopReason: string | null;
	usage: Usage;
	calls: ToolCall[];
	/** The reply's text, without thinking or calls. */
	texts: string[];
}

function fromContent(content: unknown[], stopReason: string | null, usage: Usage): ModelReply {
	const texts = replyBlocks(content).flatMap((b) => (b.type === 'text' ? [b.text] : []));
	return { content, stopReason, usage, calls: toolCalls(content), texts };
}

/**
 * One model call. The request shape must stay identical across calls in a conversation (only
 * `messages` grows), otherwise the prompt cache is lost: `tools`, `system` and `cacheTtl` are the
 * conversation's own, fixed when it was created. `cacheKey` (the conversation's id) keeps a
 * conversation's calls together in OpenAI's cache.
 */
export async function streamTurn(opts: {
	provider: Provider;
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	cacheTtl: CacheTtl;
	cacheKey: string;
	/** The conversation in btw's format; each provider's module turns it into its request. */
	messages: Message[];
	signal: AbortSignal;
	onEvent: (event: StreamEvent) => void;
}): Promise<ModelReply> {
	const { provider, cacheKey, ...request } = opts;
	if (runsOnClaudeCode(provider)) {
		throw new Error('Chats on the Claude plan run whole turns through claude-plan.ts');
	}
	if (provider === 'openai') {
		const response = await openai.streamResponse({ ...request, cacheKey });
		return fromContent(
			response.output ?? [],
			openai.stopReason(response),
			openai.summarizeUsage(response.usage)
		);
	}
	const message = await anthropic.streamTurn(request);
	return fromContent(message.content, message.stop_reason, summarizeAnthropicUsage(message.usage));
}

export function summarizeAnthropicUsage(usage: Anthropic.Usage): Usage {
	return {
		input: usage.input_tokens,
		cacheRead: usage.cache_read_input_tokens ?? 0,
		cacheWrite: usage.cache_creation_input_tokens ?? 0,
		output: usage.output_tokens
	};
}

/**
 * One short exchange at low effort, not streamed, for chores like naming a chat. `text` is null
 * when the reply didn't finish (a refusal, or cut off).
 */
export async function quickReply(opts: {
	provider: Provider;
	model: string;
	system: string;
	input: string;
	maxTokens: number;
	timeoutMs: number;
}): Promise<{ text: string | null; usage: Usage }> {
	if (runsOnClaudeCode(opts.provider)) return claudePlan.quickReply(opts);
	if (opts.provider === 'openai') {
		const response = await openai.createResponse(opts);
		const usage = openai.summarizeUsage(response.usage);
		if (openai.stopReason(response) !== 'end_turn') return { text: null, usage };
		return { text: textOf(response.output ?? []), usage };
	}
	const reply = await anthropic.createMessage(opts);
	const usage = summarizeAnthropicUsage(reply.usage);
	if (reply.stop_reason !== 'end_turn') return { text: null, usage };
	return { text: textOf(reply.content), usage };
}

function textOf(content: unknown[]): string {
	return replyBlocks(content)
		.flatMap((b) => (b.type === 'text' ? [b.text] : []))
		.join('');
}

/**
 * What an uploaded PDF costs in every request of a conversation on this model. The provider
 * reads the whole document, so it also throws for PDFs it can't use (encrypted, too many pages).
 */
export function countDocumentTokens(
	provider: Provider,
	model: string,
	fileId: string
): Promise<number> {
	if (runsOnClaudeCode(provider)) {
		return Promise.reject(new Error('Chats on the Claude plan get PDFs as files, not documents'));
	}
	return provider === 'openai'
		? openai.countDocumentTokens(model, fileId)
		: anthropic.countDocumentTokens(model, fileId);
}

/**
 * Throws if the provider doesn't know the model. Null when its window isn't known. For the
 * Claude plan, it checks that Claude Code is here and signed in to one: it has no models API, and
 * whether it takes the model shows at the chat's first reply.
 */
export async function fetchContextWindow(
	provider: Provider,
	model: string
): Promise<number | null> {
	if (runsOnClaudeCode(provider)) {
		await claudePlan.checkClaudePlan();
		return null;
	}
	return provider === 'openai'
		? openai.fetchContextWindow(model)
		: anthropic.fetchContextWindow(model);
}

export function describeApiError(err: unknown): string {
	if (err instanceof claudePlan.ClaudePlanError) return err.message;
	return openai.isOpenAIError(err) ? openai.describeApiError(err) : anthropic.describeApiError(err);
}

/** The API's own message, without the status and JSON around it: for notes shown to the model. */
export function shortApiError(err: unknown): string {
	if (err instanceof claudePlan.ClaudePlanError) return err.message;
	return openai.isOpenAIError(err) ? openai.shortApiError(err) : anthropic.shortApiError(err);
}

export function isAbortError(err: unknown): boolean {
	return (
		anthropic.isAbortError(err) || openai.isAbortError(err) || claudePlan.isPlanAbortError(err)
	);
}
