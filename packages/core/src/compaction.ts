import { supportsCompaction } from './anthropic.ts';
import { readConfig, updateConfig } from './config.ts';
import {
	awaitsReply,
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
	if (from > 0 && calls.length < 2) return false;
	return contextSince(rows) >= COMPACT_SHARE * window;
}

/** What the latest call since the latest summary read and wrote, in tokens, or 0 before one. */
function contextSince(rows: MessageRow[]): number {
	const last = rows
		.slice(compactedFrom(rows))
		.findLast((row) => row.role === 'assistant' && row.usage);
	if (!last?.usage) return 0;
	const usage = JSON.parse(last.usage) as Usage;
	return promptTokens(usage) + usage.output;
}

/**
 * Why the runner has the model summarize the conversation: it nears the window, someone in the
 * chat asked, or it has been quiet for the minutes Models & keys says (compactWhenIdle).
 */
export type SummaryReason = 'window' | 'asked' | 'idle';

/** What the runner asks the chat's model, as one more message after the conversation. */
export function summaryRequest(reason: SummaryReason): string {
	const why = {
		window:
			'This conversation has grown too long for your context window, so it is being summarized now.',
		asked:
			'Someone in the chat asked to summarize the conversation so far, so it is being summarized now.',
		idle: 'This conversation has been quiet for a while, so it is being summarized now, to keep it short for when it goes on.'
	}[reason];
	return `[${why} The summary will take the place of everything above, which you won't see again. Don't run any commands: reply only with the summary, in the conversation's language. Write down everything you will need to go on as if nothing was lost: who asked for what and what they prefer, what was done and found, decisions made, the state of any work in progress and its next steps, and the exact details (names, paths, numbers, commands). Wrap the summary in <summary></summary>.]`;
}

/** The longest a chat can be quiet before it's summarized, in minutes: a week. */
export const MAX_IDLE_MINUTES = 7 * 24 * 60;

/** Minutes a chat stays quiet before it's summarized, or null when quiet chats aren't. */
export function idleCompactionMinutes(): number | null {
	try {
		return readConfig().compactWhenIdle ?? null;
	} catch {
		return null; // not set up yet
	}
}

/** Whole minutes from 1 to MAX_IDLE_MINUTES, or null to summarize quiet chats no more. */
export function saveIdleCompaction(minutes: number | null): void {
	if (
		minutes !== null &&
		!(Number.isInteger(minutes) && minutes >= 1 && minutes <= MAX_IDLE_MINUTES)
	) {
		throw new Error(`Minutes are a whole number from 1 to ${MAX_IDLE_MINUTES}`);
	}
	updateConfig((c) => {
		if (minutes === null) delete c.compactWhenIdle;
		else c.compactWhenIdle = minutes;
	});
}

/**
 * A quiet chat is summarized only from this size on, in tokens: below it, the summary saves the
 * next reply little and would cost a call and the chat's details.
 */
export const MIN_IDLE_CONTEXT = 20_000;

/**
 * Whether a chat that went quiet gets summarized: one people see (not a background run nobody
 * continued, or a subagent's), not on the Claude plan, that waits on nobody's answer, with a reply
 * since the latest summary, and big enough to be worth it (MIN_IDLE_CONTEXT).
 */
export function idleCompactable(
	conv: Pick<Conversation, 'provider' | 'hidden'>,
	rows: MessageRow[]
): boolean {
	if (conv.hidden || isAgentPlan(conv.provider)) return false;
	if (awaitsReply(rows) || !hasNewReplies(rows)) return false;
	return contextSince(rows) >= MIN_IDLE_CONTEXT;
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
