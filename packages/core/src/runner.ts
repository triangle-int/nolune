import { EventEmitter } from 'node:events';
import { mkdirSync, rmSync } from 'node:fs';
import type Anthropic from '@anthropic-ai/sdk';
import { describeApiError, isAbortError, streamTurn, type StreamEvent } from './anthropic.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	foundText,
	getConversation,
	insertQueued,
	lastCommittedRow,
	listAllConversationIds,
	queuedRows,
	rebuildSystemPrompt,
	replaceTitle,
	requestMessages,
	setHidden,
	setTitle,
	summarizeUsage,
	toDisplay,
	toMessageParam,
	touchConversation,
	type Conversation,
	type DisplayMessage,
	type MessageRow
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { profile } from './db/schema.ts';
import { eq } from 'drizzle-orm';
import { findUploads, prepareMessage, viewedImageBlocks } from './attachments.ts';
import { folderContextFor } from './folders.ts';
import { createViewDir, imageUse, readViewedImages, type ImageUse } from './images.ts';
import { copyReplyMedia, listMedia, mediaByMessage, type PreparedMedia } from './media.ts';
import { profileDir } from './paths.ts';
import { getProfile } from './profiles.ts';
import { RUN_COMMAND_TOOL, commandEnv, parseRunCommandInput, runCommand } from './run-command.ts';
import { TITLE_LIMIT, suggestTitle, typedTitle } from './titles.ts';
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
	| { type: 'tool_output'; id: string; chunk: string }
	| { type: 'title'; title: string };

export interface Snapshot {
	title: string;
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
	const media = mediaByMessage(conversationId);
	return {
		title: getConversation(conversationId)?.title ?? '',
		running: st.running,
		error: st.error,
		messages: committedRows(conversationId).map((row) => toDisplay(row, media.get(row.id))),
		queued: queuedRows(conversationId).map((row) => toDisplay(row, media.get(row.id))),
		live: st.live,
		toolOutput: st.toolOutput
	};
}

function emitQueued(conversationId: string): void {
	emit(conversationId, {
		type: 'queued',
		queued: queuedRows(conversationId).map((row) => toDisplay(row, listMedia(row.id)))
	});
}

function commitQueued(conversationId: string): void {
	const rows = commitQueuedRows(conversationId);
	if (rows.length === 0) return;
	for (const row of rows) {
		emit(conversationId, { type: 'message', message: toDisplay(row, listMedia(row.id)) });
	}
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

/** Sends to a conversation happen one at a time, so attachments count against its limits in order. */
const sending = new Map<string, Promise<unknown>>();

/**
 * Queues a message; it joins the transcript at the agent's next step (steering) or starts a
 * turn. `uploadIds`: files the sender attached in the composer, in order.
 */
export async function sendMessage(
	conversationId: string,
	sender: { id: string; name: string },
	text: string,
	uploadIds: string[] = []
): Promise<void> {
	const previous = sending.get(conversationId) ?? Promise.resolve();
	const send = previous
		.catch(() => {})
		.then(() => queueMessage(conversationId, sender, text, uploadIds));
	sending.set(conversationId, send);
	try {
		await send;
	} finally {
		if (sending.get(conversationId) === send) sending.delete(conversationId);
	}
}

async function queueMessage(
	conversationId: string,
	sender: { id: string; name: string },
	text: string,
	uploadIds: string[]
): Promise<void> {
	const trimmed = text.trim();
	if (!trimmed && !uploadIds.length) throw new Error('Message is empty');
	const conv = getConversation(conversationId);
	if (!conv) throw new Error('No such conversation');
	const uploads = findUploads(conv.profileId, sender.id, uploadIds);
	let attachments: Parameters<typeof insertQueued>[0]['attachments'];
	if (uploads.length) {
		const slug = profileSlug(conv.profileId);
		if (!slug) throw new Error('The profile no longer exists');
		const prepared = await prepareMessage({
			conv,
			profileSlug: slug,
			senderName: sender.name,
			text: trimmed,
			uploads,
			earlier: [...committedRows(conversationId), ...queuedRows(conversationId)]
		});
		attachments = {
			content: prepared.content,
			files: prepared.attachments,
			media: prepared.media,
			uploadIds
		};
	}
	// Writing into a background run turns it into a normal conversation.
	if (conv.hidden) setHidden(conversationId, false);
	insertQueued({
		conversationId,
		senderId: sender.id,
		senderName: sender.name,
		text: trimmed,
		attachments
	});
	// The first message stands in as the title until the model has named the chat. A message
	// with only files is named after them.
	const opening = trimmed || uploads.map((u) => u.name).join(', ');
	const placeholder = conv.title ? undefined : opening.slice(0, TITLE_LIMIT);
	touchConversation(conversationId, placeholder);
	emitQueued(conversationId);
	kick(conversationId);
	if (placeholder !== undefined) nameConversation(conv, opening, placeholder);
}

/** Asks the chat's model for a title in the background; the placeholder stays if that fails. */
function nameConversation(conv: Conversation, text: string, placeholder: string): void {
	suggestTitle(conv.model, text)
		.then(({ title, usage }) => {
			console.log(
				`[btw] ${conv.id.slice(0, 8)} title ${conv.model} in=${usage.input} out=${usage.output}${title ? '' : ' (none)'}`
			);
			if (!title || !replaceTitle(conv.id, placeholder, title)) return;
			emit(conv.id, { type: 'title', title });
		})
		.catch((err) => {
			console.error(`[btw] ${conv.id.slice(0, 8)} could not name the chat:`, describeApiError(err));
		});
}

/**
 * Renames the chat for everyone who has it open. A title the model is still thinking of for a
 * new chat doesn't replace it (replaceTitle).
 */
export function renameConversation(conversationId: string, title: string): void {
	const cleaned = typedTitle(title);
	setTitle(conversationId, cleaned);
	emit(conversationId, { type: 'title', title: cleaned });
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

function profileSlug(profileId: string): string | undefined {
	return getDb().select({ slug: profile.slug }).from(profile).where(eq(profile.id, profileId)).get()
		?.slug;
}

function toolResult(
	id: string,
	text: string,
	isError: boolean,
	/** Images from `btw view`, with their labels. They follow the command's output. */
	attachments: (Anthropic.TextBlockParam | Anthropic.ImageBlockParam)[] = []
): Anthropic.ToolResultBlockParam {
	const content = attachments.length ? [{ type: 'text' as const, text }, ...attachments] : text;
	return { type: 'tool_result', tool_use_id: id, content, ...(isError ? { is_error: true } : {}) };
}

async function runToolCall(
	conv: Conversation,
	call: Anthropic.ToolUseBlock,
	stopReason: Anthropic.Message['stop_reason'],
	signal: AbortSignal,
	st: State,
	/** Images already in the conversation; grows by what this call attaches. */
	images: ImageUse
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

	const slug = profileSlug(conv.profileId);
	if (!slug) return toolResult(call.id, 'Not run: the profile no longer exists.', true);
	const dir = profileDir(slug);
	mkdirSync(dir, { recursive: true });

	st.toolOutput = { id: call.id, text: '' };
	// `btw view` in this command leaves images here, to be attached to its result.
	const viewDir = createViewDir(images);
	try {
		const result = await runCommand(input, {
			defaultCwd: dir,
			env: commandEnv({
				BTW_PROFILE: slug,
				BTW_PROFILE_DIR: dir,
				BTW_CONVERSATION_ID: conv.id,
				BTW_VIEW_DIR: viewDir
			}),
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
		const attachments = await viewedImageBlocks(conv.provider, readViewedImages(viewDir), images);
		return toolResult(call.id, result.content, result.isError, attachments);
	} finally {
		rmSync(viewDir, { recursive: true, force: true });
	}
}

/**
 * The chat with a system prompt that has its folder as it is now. When the chat moved to another
 * folder, or its folder's instructions or files changed, the prompt is built again, which costs
 * one prompt cache miss. Only between turns: in the middle of one, the model is still working
 * under the prompt it started with, and its latest thinking must go back with the tool results.
 */
export function withCurrentFolder(conv: Conversation, rows: MessageRow[]): Conversation {
	const lastReply = rows.findLast((row) => row.role === 'assistant');
	if (
		lastReply &&
		(JSON.parse(lastReply.content) as Anthropic.ContentBlock[]).some((b) => b.type === 'tool_use')
	) {
		return conv;
	}
	const owner = getProfile(conv.profileId);
	if (!owner) return conv;
	const context = folderContextFor(owner, conv.folderId);
	if (context === conv.folderContext) return conv;
	console.log(`[btw] ${conv.id.slice(0, 8)} folder changed, system prompt built again`);
	return rebuildSystemPrompt(conv, owner, context, lastReply?.seq ?? null);
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
			const stored = getConversation(conversationId);
			if (!stored) return;
			commitQueued(conversationId);
			const rows = committedRows(conversationId);
			if (rows.at(-1)?.role !== 'user') return;
			const conv = withCurrentFolder(stored, rows);

			const abort = new AbortController();
			st.abort = abort;
			const messages = requestMessages(rows, conv.promptChangedAtSeq);
			let reply: Anthropic.Message;
			try {
				reply = await streamTurn({
					model: conv.model,
					effort: conv.effort,
					system: conv.systemPrompt,
					messages,
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

			// Pictures and files the reply links to are copied before it's saved (the live reply
			// stays on screen meanwhile), so the saved reply never points at a missing copy. Web
			// pictures only if their link appeared in what the model read before this reply.
			const texts = reply.content.flatMap((b) => (b.type === 'text' ? [b.text] : []));
			const slug = profileSlug(conv.profileId);
			const media: PreparedMedia[] = slug
				? await copyReplyMedia(texts, profileDir(slug), abort.signal, () => foundText(rows))
				: [];
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
				usage,
				media
			});
			emit(conversationId, {
				type: 'message',
				message: toDisplay(assistantRow, media.length ? listMedia(assistantRow.id) : [])
			});
			touchConversation(conversationId);

			const calls = reply.content.filter((b): b is Anthropic.ToolUseBlock => b.type === 'tool_use');
			// No tool calls: the turn is over. Loop again in case messages arrived meanwhile.
			if (calls.length === 0) continue;

			// Queued messages may carry pictures too; they join the history at the next step.
			const images = imageUse([...messages, ...queuedRows(conversationId).map(toMessageParam)]);
			const results: Anthropic.ToolResultBlockParam[] = [];
			for (const call of calls) {
				// A call that throws still gets its result, or the reply would wait for one forever.
				const result = await runToolCall(
					conv,
					call,
					reply.stop_reason,
					abort.signal,
					st,
					images
				).catch((err: unknown) => {
					st.toolOutput = null;
					console.error(`[btw] ${conversationId.slice(0, 8)} command failed:`, err);
					const reason = err instanceof Error ? err.message : String(err);
					return toolResult(call.id, `Not finished: ${reason}`, true);
				});
				results.push(result);
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
		// `pnpm dev` runs this again when a file changes, without a restart: a loop still running
		// here answers its own calls, and a second answer would break the conversation.
		if (isRunning(id)) continue;
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
