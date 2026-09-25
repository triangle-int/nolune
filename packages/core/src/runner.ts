import { EventEmitter } from 'node:events';
import { mkdirSync } from 'node:fs';
import type Anthropic from '@anthropic-ai/sdk';
import { describeApiError, isAbortError, streamTurn, type StreamEvent } from './anthropic.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	getConversation,
	insertQueued,
	lastCommittedRow,
	listAllConversationIds,
	queuedRows,
	setHidden,
	summarizeUsage,
	toDisplay,
	toMessageParam,
	touchConversation,
	type Conversation,
	type DisplayMessage
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { profile } from './db/schema.ts';
import { eq } from 'drizzle-orm';
import { profileDir } from './paths.ts';
import { RUN_COMMAND_TOOL, commandEnv, parseRunCommandInput, runCommand } from './run-command.ts';
import { cacheHitRate } from './usage.ts';

export interface LiveBlock {
	type: 'text' | 'thinking' | 'tool';
	text: string;
	id?: string;
}

export type LiveEvent =
	| { type: 'status'; running: boolean; error: string | null }
	| { type: 'queued'; queued: DisplayMessage[] }
	| { type: 'message'; message: DisplayMessage }
	| { type: 'live_block'; index: number; block: LiveBlock }
	| { type: 'live_delta'; index: number; text: string }
	| { type: 'live_clear' }
	| { type: 'tool_output'; id: string; chunk: string };

export interface Snapshot {
	running: boolean;
	error: string | null;
	messages: DisplayMessage[];
	queued: DisplayMessage[];
	live: (LiveBlock | null)[];
	toolOutput: { id: string; text: string } | null;
}

interface State {
	running: boolean;
	abort: AbortController | null;
	stoppedBy: string | null;
	error: string | null;
	live: (LiveBlock | null)[];
	toolOutput: { id: string; text: string } | null;
	emitter: EventEmitter;
}

const LIVE_OUTPUT_LIMIT = 100_000;

const holder = globalThis as unknown as {
	__btwRunner?: Map<string, State>;
	__btwLoopEnd?: Set<LoopEndListener>;
};
const states = (holder.__btwRunner ??= new Map());

/** Called whenever a conversation's agent loop stops, with the error if a model call failed. */
export type LoopEndListener = (conversationId: string, error: string | null) => void;
const loopEndListeners = (holder.__btwLoopEnd ??= new Set());

export function onLoopEnd(listener: LoopEndListener): () => void {
	loopEndListeners.add(listener);
	return () => loopEndListeners.delete(listener);
}

function stateFor(conversationId: string): State {
	let st = states.get(conversationId);
	if (!st) {
		st = {
			running: false,
			abort: null,
			stoppedBy: null,
			error: null,
			live: [],
			toolOutput: null,
			emitter: new EventEmitter()
		};
		st.emitter.setMaxListeners(100);
		states.set(conversationId, st);
	}
	return st;
}

function emit(conversationId: string, event: LiveEvent): void {
	stateFor(conversationId).emitter.emit('event', event);
}

export function subscribe(
	conversationId: string,
	listener: (event: LiveEvent) => void
): () => void {
	const { emitter } = stateFor(conversationId);
	emitter.on('event', listener);
	return () => emitter.off('event', listener);
}

export function getSnapshot(conversationId: string): Snapshot {
	const st = stateFor(conversationId);
	return {
		running: st.running,
		error: st.error,
		messages: committedRows(conversationId).map(toDisplay),
		queued: queuedRows(conversationId).map(toDisplay),
		live: st.live,
		toolOutput: st.toolOutput
	};
}

function emitQueued(conversationId: string): void {
	emit(conversationId, { type: 'queued', queued: queuedRows(conversationId).map(toDisplay) });
}

function commitQueued(conversationId: string): void {
	const rows = commitQueuedRows(conversationId);
	if (rows.length === 0) return;
	for (const row of rows) emit(conversationId, { type: 'message', message: toDisplay(row) });
	emitQueued(conversationId);
}

function clearLive(conversationId: string): void {
	const st = stateFor(conversationId);
	st.live = [];
	st.toolOutput = null;
	emit(conversationId, { type: 'live_clear' });
}

function onStreamEvent(conversationId: string, event: StreamEvent): void {
	const st = stateFor(conversationId);
	if (event.type === 'block_start') {
		const b = event.block;
		let block: LiveBlock | null = null;
		if (b.type === 'text') block = { type: 'text', text: '' };
		else if (b.type === 'thinking') block = { type: 'thinking', text: '' };
		else if (b.type === 'tool_use') block = { type: 'tool', text: '', id: b.id };
		if (!block) return;
		st.live[event.index] = block;
		emit(conversationId, { type: 'live_block', index: event.index, block });
	} else {
		const block = st.live[event.index];
		if (!block) return;
		block.text += event.text;
		emit(conversationId, { type: 'live_delta', index: event.index, text: event.text });
	}
}

/** Queues a message; it joins the transcript at the agent's next step (steering) or starts a turn. */
export function sendMessage(
	conversationId: string,
	sender: { id: string; name: string },
	text: string
): void {
	const trimmed = text.trim();
	if (!trimmed) throw new Error('Message is empty');
	const conv = getConversation(conversationId);
	if (!conv) throw new Error('No such conversation');
	// Writing into a background run turns it into a normal conversation.
	if (conv.hidden) setHidden(conversationId, false);
	insertQueued({ conversationId, senderId: sender.id, senderName: sender.name, text: trimmed });
	touchConversation(conversationId, conv.title ? undefined : trimmed.slice(0, 80));
	emitQueued(conversationId);
	kick(conversationId);
}

export function stop(conversationId: string, byName: string): void {
	const st = stateFor(conversationId);
	if (!st.running || !st.abort) return;
	st.stoppedBy = byName;
	st.abort.abort();
}

/** Runs the agent if the transcript ends with something it hasn't answered yet. */
export function kick(conversationId: string): void {
	loop(conversationId).catch((err) => {
		console.error(`[btw] runner crashed for ${conversationId}:`, err);
	});
}

export function isRunning(conversationId: string): boolean {
	return stateFor(conversationId).running;
}

function stoppedText(st: State): string {
	return `Stopped by ${st.stoppedBy ?? 'a user'}.`;
}

function toolResult(id: string, content: string, isError: boolean): Anthropic.ToolResultBlockParam {
	return { type: 'tool_result', tool_use_id: id, content, ...(isError ? { is_error: true } : {}) };
}

async function runToolCall(
	conv: Conversation,
	call: Anthropic.ToolUseBlock,
	stopReason: Anthropic.Message['stop_reason'],
	signal: AbortSignal,
	st: State
): Promise<Anthropic.ToolResultBlockParam> {
	if (stopReason !== 'tool_use') {
		return toolResult(
			call.id,
			'Not run: the reply was cut off before this call was complete.',
			true
		);
	}
	if (call.name !== RUN_COMMAND_TOOL.name)
		return toolResult(call.id, `Unknown tool "${call.name}".`, true);
	if (signal.aborted) return toolResult(call.id, `Not run. ${stoppedText(st)}`, true);
	const input = parseRunCommandInput(call.input);
	if (typeof input === 'string') return toolResult(call.id, `Invalid input: ${input}`, true);

	const slug = getDb()
		.select({ slug: profile.slug })
		.from(profile)
		.where(eq(profile.id, conv.profileId))
		.get()?.slug;
	if (!slug) return toolResult(call.id, 'Not run: the profile no longer exists.', true);
	const dir = profileDir(slug);
	mkdirSync(dir, { recursive: true });

	st.toolOutput = { id: call.id, text: '' };
	const result = await runCommand(input, {
		defaultCwd: dir,
		env: commandEnv({ BTW_PROFILE: slug, BTW_PROFILE_DIR: dir, BTW_CONVERSATION_ID: conv.id }),
		signal,
		abortReason: () => stoppedText(st),
		onOutput: (chunk) => {
			const live = st.toolOutput;
			if (!live || live.text.length >= LIVE_OUTPUT_LIMIT) return;
			const piece = chunk.slice(0, LIVE_OUTPUT_LIMIT - live.text.length);
			live.text += piece;
			emit(conv.id, { type: 'tool_output', id: call.id, chunk: piece });
		}
	});
	st.toolOutput = null;
	return toolResult(call.id, result.content, result.isError);
}

async function loop(conversationId: string): Promise<void> {
	const st = stateFor(conversationId);
	if (st.running) return; // the running loop picks up new messages at its next step
	st.running = true;
	st.error = null;
	st.stoppedBy = null;
	emit(conversationId, { type: 'status', running: true, error: null });

	try {
		for (;;) {
			const conv = getConversation(conversationId);
			if (!conv) return;
			commitQueued(conversationId);
			const rows = committedRows(conversationId);
			if (rows.at(-1)?.role !== 'user') return;

			const abort = new AbortController();
			st.abort = abort;
			let reply: Anthropic.Message;
			try {
				reply = await streamTurn({
					model: conv.model,
					effort: conv.effort,
					system: conv.systemPrompt,
					messages: rows.map(toMessageParam),
					signal: abort.signal,
					onEvent: (event) => onStreamEvent(conversationId, event)
				});
			} catch (err) {
				clearLive(conversationId);
				if (abort.signal.aborted || isAbortError(err)) {
					// The partial reply is dropped; waiting messages join the transcript unanswered.
					commitQueued(conversationId);
					return;
				}
				st.error = describeApiError(err);
				console.error(`[btw] ${conversationId.slice(0, 8)} model call failed:`, err);
				return;
			}
			clearLive(conversationId);

			const usage = summarizeUsage(reply.usage);
			console.log(
				`[btw] ${conversationId.slice(0, 8)} ${conv.model} in=${usage.input} cache_read=${usage.cacheRead} cache_write=${usage.cacheWrite} hit=${Math.floor(cacheHitRate(usage) * 100)}% out=${usage.output} stop=${reply.stop_reason}`
			);
			const assistantRow = appendRow({
				conversationId,
				role: 'assistant',
				kind: 'assistant',
				content: JSON.stringify(reply.content),
				stopReason: reply.stop_reason,
				usage
			});
			emit(conversationId, { type: 'message', message: toDisplay(assistantRow) });
			touchConversation(conversationId);

			const calls = reply.content.filter((b): b is Anthropic.ToolUseBlock => b.type === 'tool_use');
			// No tool calls: the turn is over. Loop again in case messages arrived meanwhile.
			if (calls.length === 0) continue;

			const results: Anthropic.ToolResultBlockParam[] = [];
			for (const call of calls) {
				results.push(await runToolCall(conv, call, reply.stop_reason, abort.signal, st));
			}
			const resultsRow = appendRow({
				conversationId,
				role: 'user',
				kind: 'tool_results',
				content: JSON.stringify(results)
			});
			emit(conversationId, { type: 'message', message: toDisplay(resultsRow) });
			if (abort.signal.aborted) {
				commitQueued(conversationId);
				return;
			}
		}
	} finally {
		st.running = false;
		st.abort = null;
		st.live = [];
		st.toolOutput = null;
		emit(conversationId, { type: 'status', running: false, error: st.error });
		for (const listener of loopEndListeners) {
			try {
				listener(conversationId, st.error);
			} catch (err) {
				console.error(`[btw] loop-end listener failed for ${conversationId}:`, err);
			}
		}
	}
}

/**
 * After a restart: answer tool calls that never got a result (appending, never editing), then
 * resume conversations that have queued messages.
 */
export function recoverAfterRestart(): void {
	for (const id of listAllConversationIds()) {
		const last = lastCommittedRow(id);
		if (last?.kind === 'assistant') {
			const calls = (JSON.parse(last.content) as Anthropic.ContentBlock[]).filter(
				(b): b is Anthropic.ToolUseBlock => b.type === 'tool_use'
			);
			if (calls.length) {
				appendRow({
					conversationId: id,
					role: 'user',
					kind: 'tool_results',
					content: JSON.stringify(
						calls.map((c) =>
							toolResult(c.id, 'Not finished: the gateway restarted while this was running.', true)
						)
					)
				});
			}
		}
		if (queuedRows(id).length) kick(id);
	}
}
