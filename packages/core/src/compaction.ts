import { supportsCompaction } from './anthropic.ts';
import {
	compactedFrom,
	compactionSummary,
	type Conversation,
	type MessageRow,
	type Usage
} from './conversations.ts';
import type { ModelReply, Provider } from './models.ts';
import { isAgentPlan } from './plans.ts';
import { promptTokens } from './usage.ts';

/*
 * Compaction: once a conversation fills 85% of its model's context window, the model summarizes
 * it, and the model calls after that start from the summary rather than the whole conversation.
 * Rows are still never edited or deleted (see Prompt caching in DESIGN.md): the chat shows
 * everything, the summary as one more step, and requestMessages leaves out what came before it.
 *
 * Claude does it on the server (compaction at a token threshold, anthropic.ts): the request that
 * reaches the threshold writes the summary first, as a `compaction` block that starts its reply,
 * and goes on from it. That reply goes back as it came, and the API takes the block in place of
 * everything before it. Chats on other models are summarized by the runner before the call that
 * would reach the threshold: it asks the chat's model, with the same request and one more message,
 * so the provider's cache still holds all but that message, and saves the summary as a row of its
 * own (`compaction`), which goes to the model as a message.
 *
 * Chats on the Claude plan aren't summarized here: Claude Code keeps their conversation and
 * compacts it itself.
 */

/** The share of the context window a conversation fills before it's summarized. */
export const COMPACT_SHARE = 0.85;

/** Claude's threshold can't be lower. */
const MIN_COMPACT_AT = 50_000;

/** The window Claude's threshold is figured from when the preset doesn't know the model's. */
const DEFAULT_CONTEXT_WINDOW = 200_000;

/** Whether the chat's model summarizes the conversation on the server. */
export function compactsOnServer(provider: Provider, model: string): boolean {
	return provider === 'anthropic' && supportsCompaction(model);
}

/** The request size, in tokens, at which Claude summarizes the conversation on the server. */
export function serverCompactAt(contextWindow: number | null): number {
	const window = contextWindow ?? DEFAULT_CONTEXT_WINDOW;
	return Math.max(MIN_COMPACT_AT, Math.floor(COMPACT_SHARE * window));
}

/**
 * Whether the runner summarizes the conversation before its next model call: the model doesn't on
 * the server, its window is known, and the latest call read and wrote 85% of it or more. Never
 * right after a summary: a window that the conversation can't fit even then would only get one
 * summary after another.
 */
export function needsCompaction(
	conv: Pick<Conversation, 'provider' | 'model' | 'contextWindow'>,
	rows: MessageRow[]
): boolean {
	const window = conv.contextWindow;
	if (!window || isAgentPlan(conv.provider) || compactsOnServer(conv.provider, conv.model)) {
		return false;
	}
	const from = compactedFrom(rows);
	const calls = rows.slice(from).filter((row) => row.role === 'assistant' && row.usage);
	const last = calls.at(-1);
	if (!last?.usage || (from > 0 && calls.length < 2)) return false;
	const usage = JSON.parse(last.usage) as Usage;
	return promptTokens(usage) + usage.output >= COMPACT_SHARE * window;
}

/**
 * What the runner asks the chat's model, as one more message after the conversation: because the
 * conversation nears its window, or because someone in the chat asked for it (`asked`).
 */
export function summaryRequest(asked: boolean): string {
	const why = asked
		? 'Someone in the chat asked to summarize the conversation so far, so it is being summarized now.'
		: 'This conversation has grown too long for your context window, so it is being summarized now.';
	return `[${why} The summary will take the place of everything above, which you won't see again. Don't run any commands: reply only with the summary, in the conversation's language. Write down everything you will need to go on as if nothing was lost: who asked for what and what they prefer, what was done and found, decisions made, the state of any work in progress and its next steps, and the exact details (names, paths, numbers, commands). Wrap the summary in <summary></summary>.]`;
}

/**
 * Whether there's anything to summarize: a reply since the latest summary. The one that starts with
 * Claude's summary doesn't count.
 */
export function hasNewReplies(rows: MessageRow[]): boolean {
	const from = compactedFrom(rows);
	const cut = rows[from];
	const since = cut && compactionSummary(cut) !== null ? rows.slice(from + 1) : rows;
	return since.some((row) => row.role === 'assistant');
}

/**
 * The summary in the model's reply to summaryRequest, or null if it didn't write one. Without the
 * tags, the whole reply, unless the model went on to run a command: then its text was only what
 * it said first.
 */
export function summaryOf(reply: Pick<ModelReply, 'texts' | 'stopReason'>): string | null {
	if (reply.stopReason === 'max_tokens' || reply.stopReason === 'refusal') return null;
	const text = reply.texts.join('\n\n');
	const wrapped = /<summary>([\s\S]*?)<\/summary>/i.exec(text)?.[1];
	if (wrapped === undefined && reply.stopReason === 'tool_use') return null;
	return (wrapped ?? text.replace(/<\/?summary>/gi, '')).trim() || null;
}
