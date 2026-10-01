import { randomUUID } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { mkdirSync, rmSync } from 'node:fs';
import { isAgentPlan, type AgentPlan } from './plans.ts';
import {
	describeApiError,
	isAbortError,
	pictureTypes,
	planSessionProblem,
	readableMessages,
	runPlanTurn,
	streamTurn,
	type Effort,
	type ModelReply,
	type StreamEvent,
	type ToolCall
} from './models.ts';
import {
	backgroundCommands,
	startBackgroundCommand,
	takeInterruptedBackgroundCommands,
	type BackgroundCommand
} from './background.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	foundText,
	getConversation,
	insertQueued,
	insertQueuedNotice,
	isSubagentConversation,
	lastCommittedRow,
	listAllConversationIds,
	plainText,
	queuedRows,
	rebuildSystemPrompt,
	replaceTitle,
	readRow,
	requestMessages,
	rowCalls,
	setCommandMode,
	setEffort,
	setHidden,
	setPreset,
	setProviderSession,
	setTitle,
	toDisplay,
	toolsFor,
	touchConversation,
	type Conversation,
	type DisplayMessage,
	type MessageRow
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { profile } from './db/schema.ts';
import { eq } from 'drizzle-orm';
import {
	messageText,
	type Block,
	type ImageBlock,
	type TextBlock,
	type ToolResultBlock
} from './format.ts';
import { findUploads, prepareMessage, viewedImageBlocks } from './attachments.ts';
import {
	BLOCK_STREAK_LIMIT,
	BLOCK_TOTAL_LIMIT,
	SAFETY_STOP,
	blockedText,
	chatCommandMode,
	checkCommand,
	commandMode,
	refusedText,
	type CommandMode
} from './command-safety.ts';
import { folderContextFor } from './folders.ts';
import { readSoul } from './soul.ts';
import { createViewDir, imageUse, readViewedImages, type ImageUse } from './images.ts';
import {
	copyReplyMedia,
	copyViewedImages,
	listMedia,
	mediaByMessage,
	type PreparedMedia
} from './media.ts';
import { memoryLooks, type DisplayMemoryLook } from './memory-changes.ts';
import { profileCards } from './memory-cards.ts';
import { memberWords } from './memory-people.ts';
import { recallFor } from './memory-search.ts';
import { profileDir } from './paths.ts';
import { hasFileStore, resolveFiles } from './provider-files.ts';
import { getProfile, noticeProfileChanges } from './profiles.ts';
import {
	RUN_COMMAND_TOOL,
	commandEnv,
	parseRunCommandInput,
	resolveCwd,
	runCommand,
	type RunCommandInput,
	type RunCommandResult
} from './run-command.ts';
import { SubagentError, activeSubagents, listSubagents } from './subagents.ts';
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
	/** `replacesLive`: the live reply, now saved, which takes the live blocks' place. */
	| { type: 'message'; message: DisplayMessage; replacesLive?: true }
	| { type: 'live_block'; index: number; block: LiveBlock }
	| { type: 'live_delta'; index: number; text: string }
	| { type: 'live_clear' }
	| { type: 'tool_output'; id: string; chunk: string }
	| { type: 'title'; title: string }
	| { type: 'model'; model: ChatModel }
	| { type: 'commands'; commands: ChatCommands }
	| { type: 'background'; background: BackgroundItem[] }
	/** What the note-taker saved from the chat (memory-changes.ts), all of it. */
	| { type: 'memory'; memory: DisplayMemoryLook[] }
	/** Everyone writing in the chat's composer right now, in the order they started. */
	| { type: 'typing'; typing: Typist[] };

/** Someone writing a message in a chat, which the others who have it open see. */
export interface Typist {
	/** Their user id. */
	id: string;
	name: string;
}

/**
 * How long someone counts as typing after their page last said so. While they type it says so
 * again every few seconds, and that they stopped when they do; this is for a page that closed or
 * lost its connection first.
 */
export const TYPING_TTL_MS = 8_000;

/** The model and reasoning level a conversation's next model call uses. */
export interface ChatModel {
	/** Null once the preset was removed; the conversation keeps its model. */
	presetId: string | null;
	presetName: string;
	provider: Conversation['provider'];
	effort: Effort;
	contextWindow: number | null;
}

function chatModel(conv: Conversation): ChatModel {
	const { presetId, presetName, provider, effort, contextWindow } = conv;
	return { presetId, presetName, provider, effort, contextWindow };
}

/** How a chat's commands run (command-safety.ts), for its composer. */
export interface ChatCommands {
	/** The chat's own choice; null when it goes by Models & keys. */
	mode: CommandMode | null;
	/** What Models & keys says, which the chat goes by without a choice of its own. */
	fallback: CommandMode;
}

function chatCommands(conv: Pick<Conversation, 'commandMode'> | undefined): ChatCommands {
	return { mode: conv?.commandMode ?? null, fallback: commandMode() };
}

/** Work of the conversation's agent that goes on while it does other things, or nothing. */
export type BackgroundItem =
	| {
			kind: 'command';
			/** The run_command call that started it. */
			id: string;
			summary: string | null;
			command: string;
			startedAt: number;
	  }
	| {
			kind: 'subagent';
			id: string;
			/** What the agent calls it, like agent-1. */
			name: string;
			/** Its own chat, which people can open. */
			conversationId: string;
			status: 'pending' | 'running' | 'stopping';
			startedAt: number;
	  };

export interface Snapshot {
	title: string;
	/** Null once the conversation was deleted. */
	model: ChatModel | null;
	commands: ChatCommands;
	running: boolean;
	error: string | null;
	messages: DisplayMessage[];
	queued: DisplayMessage[];
	live: (LiveBlock | null)[];
	toolOutput: { id: string; text: string } | null;
	background: BackgroundItem[];
	/** What the note-taker saved from it. */
	memory: DisplayMemoryLook[];
	/** Who is writing in it right now, the person looking at it included. */
	typing: Typist[];
}

/**
 * What auto mode blocked (command-safety.ts) in the current loop, since a person last wrote. Too
 * many blocks and the agent is told to stop and ask; a call it makes after that ends the loop.
 */
interface SafetyTally {
	/** The last message from a person it counts from: a newer one starts again. */
	since: number | null;
	/** Blocked in a row, and in all. */
	streak: number;
	total: number;
	/** Calls refused unchecked because the agent was told to stop. */
	refused: number;
}

function freshTally(since: number | null = null): SafetyTally {
	return { since, streak: 0, total: 0, refused: 0 };
}

interface State {
	running: boolean;
	abort: AbortController | null;
	stoppedBy: string | null;
	error: string | null;
	live: (LiveBlock | null)[];
	toolOutput: { id: string; text: string } | null;
	safety: SafetyTally;
	/** Who is typing, by user id, each until their TYPING_TTL_MS runs out. */
	typing: Map<string, { name: string; expiry: NodeJS.Timeout }>;
	emitter: EventEmitter;
}

const LIVE_OUTPUT_LIMIT = 100_000;

const holder = globalThis as unknown as {
	__noluneRunner?: Map<string, State>;
	__noluneLoopEnd?: Set<LoopEndListener>;
	__noluneCommandEnd?: Set<() => void>;
	__noluneRunningChange?: Set<RunningChangeListener>;
};
const states = (holder.__noluneRunner ??= new Map());

/** Called whenever a conversation's agent loop stops, with the error if a model call failed. */
export type LoopEndListener = (conversationId: string, error: string | null) => void;
const loopEndListeners = (holder.__noluneLoopEnd ??= new Set());

export function onLoopEnd(listener: LoopEndListener): () => void {
	loopEndListeners.add(listener);
	return () => loopEndListeners.delete(listener);
}

/**
 * Called after every command the agent ran (not in the background), so what it asked for with
 * `nolune agent` happens right away rather than at the scheduler's next tick.
 */
const commandEndListeners = (holder.__noluneCommandEnd ??= new Set());

export function onCommandEnd(listener: () => void): () => void {
	commandEndListeners.add(listener);
	return () => commandEndListeners.delete(listener);
}

function commandEnded(): void {
	for (const listener of commandEndListeners) {
		try {
			listener();
		} catch (err) {
			console.error('[nolune] command-end listener failed:', err);
		}
	}
}

/** Called whenever any conversation's agent loop starts or stops. */
export type RunningChangeListener = (conversationId: string, running: boolean) => void;
const runningChangeListeners = (holder.__noluneRunningChange ??= new Set());

export function onRunningChange(listener: RunningChangeListener): () => void {
	runningChangeListeners.add(listener);
	return () => runningChangeListeners.delete(listener);
}

function runningChanged(conversationId: string, running: boolean): void {
	for (const listener of runningChangeListeners) {
		try {
			listener(conversationId, running);
		} catch (err) {
			console.error(`[nolune] running listener failed for ${conversationId}:`, err);
		}
	}
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
			safety: freshTally(),
			typing: new Map(),
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
	const conv = getConversation(conversationId);
	return {
		title: conv?.title ?? '',
		model: conv ? chatModel(conv) : null,
		commands: chatCommands(conv),
		running: st.running,
		error: st.error,
		messages: committedRows(conversationId).map((row) => toDisplay(row, media.get(row.id))),
		queued: queuedRows(conversationId).map((row) => toDisplay(row, media.get(row.id))),
		live: st.live,
		toolOutput: st.toolOutput,
		background: backgroundItems(conversationId),
		memory: memoryLooks(conversationId),
		typing: typists(st)
	};
}

function typists(st: State): Typist[] {
	return [...st.typing].map(([id, { name }]) => ({ id, name }));
}

/**
 * Whether `person` is writing in the chat, for everyone who has it open. Someone typing is told
 * again every few seconds; that only tells the others when their name changed. They stop by
 * saying so, by sending their message, or TYPING_TTL_MS after they last said they were typing.
 */
export function setTyping(conversationId: string, person: Typist, typing: boolean): void {
	const st = stateFor(conversationId);
	const known = st.typing.get(person.id);
	if (known) clearTimeout(known.expiry);
	if (typing) {
		const expiry = setTimeout(() => setTyping(conversationId, person, false), TYPING_TTL_MS);
		expiry.unref();
		st.typing.set(person.id, { name: person.name, expiry });
		if (known?.name === person.name) return;
	} else {
		if (!known) return;
		st.typing.delete(person.id);
	}
	emit(conversationId, { type: 'typing', typing: typists(st) });
}

/** The subagents a command waits for with `nolune agent watch`, if it does. */
function watchedSubagents(command: string): string[] {
	return [...command.matchAll(/\bnolune\s+agent\s+watch\s+([a-z0-9][a-z0-9-]*)/gi)].map((m) =>
		m[1].toLowerCase()
	);
}

function backgroundItems(conversationId: string): BackgroundItem[] {
	// A `nolune agent watch` running in the background is the same work as the subagent it waits
	// for, which is listed on its own. Any of the chat's subagents, not only working ones: a watch
	// still takes a moment to notice that its subagent finished.
	const names = new Set(listSubagents(conversationId).map((s) => s.name));
	const commands = backgroundCommands(conversationId)
		.filter((c) => {
			const watched = watchedSubagents(c.command);
			return !watched.length || !watched.every((name) => names.has(name));
		})
		.map((c): BackgroundItem => ({
			kind: 'command',
			id: c.id,
			summary: c.summary,
			command: c.command,
			startedAt: c.startedAt
		}));
	const subagents = activeSubagents(conversationId).map((s): BackgroundItem => ({
		kind: 'subagent',
		id: s.id,
		name: s.name,
		conversationId: s.conversationId,
		status: s.status as 'pending' | 'running' | 'stopping',
		startedAt: s.updatedAt.getTime()
	}));
	return [...commands, ...subagents];
}

/** Tells the conversation's open chats what the note-taker saved from it, after a save or undo. */
export function refreshMemoryLooks(conversationId: string): void {
	emit(conversationId, { type: 'memory', memory: memoryLooks(conversationId) });
}

/** Tells the conversation's open chats that its background work changed. */
export function refreshBackground(conversationId: string): void {
	emit(conversationId, { type: 'background', background: backgroundItems(conversationId) });
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
		const block: LiveBlock = { ...event.block, text: '' };
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
	if (isSubagentConversation(conversationId)) {
		throw new SubagentError(
			"This is a subagent's chat: only the agent that started it writes here."
		);
	}
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
		provider: conv.provider,
		attachments,
		recall: await recall(conv, trimmed, sender)
	});
	// The first message stands in as the title until the model has named the chat. A message
	// with only files is named after them.
	const opening = trimmed || uploads.map((u) => u.name).join(', ');
	const placeholder = conv.title ? undefined : opening.slice(0, TITLE_LIMIT);
	touchConversation(conversationId, placeholder);
	emitQueued(conversationId);
	// After the message shows, so the chat doesn't shrink for a moment in between.
	setTyping(conversationId, sender, false);
	kick(conversationId);
	if (placeholder !== undefined) nameConversation(conv, opening, placeholder);
}

/**
 * What memory has on a message, to go along with it (recallFor), leaving out what the chat
 * already has. Null when nothing matches, and when memory can't be read: that must never stop a
 * message.
 */
async function recall(
	conv: Conversation,
	text: string,
	sender: { id: string; name: string }
): Promise<string | null> {
	const slug = profileSlug(conv.profileId);
	if (!slug || !text) return null;
	try {
		const rows = [...committedRows(conv.id), ...queuedRows(conv.id)];
		const known = [conv.systemPrompt, ...rows.map((row) => messageText(readRow(row).blocks))];
		// What's about who's asking comes first, by any name their note calls them.
		const words = memberWords({ id: conv.profileId, slug }, sender.id, sender.name);
		// The members' cards too: one that changed since the chat started has news.
		const cards = profileCards(conv.profileId).map((card) => card.path);
		return await recallFor(slug, text, { sender: words, known: known.join('\n'), cards });
	} catch (err) {
		console.error(`[nolune] ${conv.id.slice(0, 8)} could not look in memory:`, err);
		return null;
	}
}

/** Asks the chat's model for a title in the background; the placeholder stays if that fails. */
function nameConversation(conv: Conversation, text: string, placeholder: string): void {
	suggestTitle(conv.provider, conv.model, text)
		.then(({ title, usage }) => {
			console.log(
				`[nolune] ${conv.id.slice(0, 8)} title ${conv.model} in=${usage.input} out=${usage.output}${title ? '' : ' (none)'}`
			);
			if (!title || !replaceTitle(conv.id, placeholder, title)) return;
			emit(conv.id, { type: 'title', title });
		})
		.catch((err) => {
			console.error(
				`[nolune] ${conv.id.slice(0, 8)} could not name the chat:`,
				describeApiError(err)
			);
		});
}

/**
 * Switches the chat to another model preset (setPreset), for everyone who has it open. Throws
 * ModelSwitchError when it can't.
 */
export function changeModel(conversationId: string, presetId: string): ChatModel {
	const model = chatModel(setPreset(conversationId, presetId));
	emit(conversationId, { type: 'model', model });
	return model;
}

/** Changes the chat's reasoning level, for everyone who has it open. Null if it's gone. */
export function changeEffort(conversationId: string, effort: Effort): ChatModel | null {
	setEffort(conversationId, effort);
	const conv = getConversation(conversationId);
	if (!conv) return null;
	const model = chatModel(conv);
	emit(conversationId, { type: 'model', model });
	return model;
}

/**
 * How the chat's commands run from its next one on, for everyone who has it open: `mode`, or
 * null to go by Models & keys. Its subagents' commands follow. Null if the chat is gone.
 */
export function changeCommandMode(
	conversationId: string,
	mode: CommandMode | null
): ChatCommands | null {
	setCommandMode(conversationId, mode);
	for (const s of listSubagents(conversationId)) setCommandMode(s.conversationId, mode);
	const conv = getConversation(conversationId);
	if (!conv) return null;
	const commands = chatCommands(conv);
	emit(conversationId, { type: 'commands', commands });
	return commands;
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
	// The first to press Stop is the one the transcript names.
	if (!st.running || !st.abort || st.abort.signal.aborted) return;
	st.stoppedBy = byName;
	st.abort.abort();
}

/** Runs the agent if the transcript ends with something it hasn't answered yet. */
export function kick(conversationId: string): void {
	loop(conversationId).catch((err) => {
		console.error(`[nolune] runner crashed for ${conversationId}:`, err);
	});
}

export function isRunning(conversationId: string): boolean {
	return stateFor(conversationId).running;
}

/** Who pressed Stop in the conversation's current or latest loop, if anyone. */
export function stoppedBy(conversationId: string): string | null {
	return stateFor(conversationId).stoppedBy;
}

/** Every conversation whose agent loop is going right now, in any profile. */
export function runningConversationIds(): string[] {
	return [...states].filter(([, st]) => st.running).map(([id]) => id);
}

function stoppedText(st: State): string {
	return `Stopped by ${st.stoppedBy ?? 'a user'}.`;
}

function profileSlug(profileId: string): string | undefined {
	return getDb().select({ slug: profile.slug }).from(profile).where(eq(profile.id, profileId)).get()
		?.slug;
}

/**
 * Copies of the pictures a command attached with `nolune view`, for the chat to show under it,
 * by the result they belong to: they're saved with the results' row (saveResults).
 */
const viewedMedia = new WeakMap<ToolResultBlock, PreparedMedia[]>();

function toolResult(
	id: string,
	text: string,
	isError: boolean,
	/** Images from `nolune view`, with their labels. They follow the command's output. */
	attachments: (TextBlock | ImageBlock)[] = []
): ToolResultBlock {
	const content = attachments.length ? [{ type: 'text' as const, text }, ...attachments] : text;
	return { type: 'tool_result', callId: id, content, isError };
}

async function runToolCall(
	conv: Conversation,
	call: ToolCall,
	stopReason: string | null,
	signal: AbortSignal,
	st: State,
	/** Images already in the conversation; grows by what this call attaches. */
	images: ImageUse
): Promise<ToolResultBlock> {
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
	const env = {
		NOLUNE_PROFILE: slug,
		NOLUNE_PROFILE_DIR: dir,
		NOLUNE_CONVERSATION_ID: conv.id
	};

	const blocked = await safetyCheck(conv, call, input, dir, signal, st);
	if (blocked) return blocked;

	if (input.background) {
		// No NOLUNE_VIEW_DIR: nothing collects the pictures of a command nobody waits for.
		const outcome = await startBackgroundCommand({
			conversationId: conv.id,
			toolUseId: call.id,
			summary: commandSummary(call),
			input,
			defaultCwd: dir,
			env: commandEnv(env),
			onEnd: backgroundCommandEnded
		});
		if ('result' in outcome) return toolResult(call.id, outcome.result.content, true);
		refreshBackground(conv.id);
		const { pid } = outcome.started;
		return toolResult(
			call.id,
			`Started in the background (process group ${pid}). When it ends, its output arrives in a message of its own that starts with "[Background command finished"; keep working or end your turn meanwhile. To stop it early: kill -TERM -${pid}`,
			false
		);
	}

	st.toolOutput = { id: call.id, text: '' };
	// `nolune view` in this command leaves images here, to be attached to its result.
	const viewDir = createViewDir(images, pictureTypes(conv.provider));
	try {
		const result = await runCommand(input, {
			defaultCwd: dir,
			env: commandEnv({ ...env, NOLUNE_VIEW_DIR: viewDir }),
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
		const viewed = await viewedImageBlocks(conv, readViewedImages(viewDir), images);
		const block = toolResult(call.id, result.content, result.isError, viewed.blocks);
		if (viewed.attached.length) {
			viewedMedia.set(block, await copyViewedImages(call.id, viewed.attached));
		}
		return block;
	} finally {
		rmSync(viewDir, { recursive: true, force: true });
		// The command may have changed the profile (`nolune profile avatar`): show it right away.
		noticeProfileChanges();
		commandEnded();
	}
}

/** The promise's value, or null as soon as the signal aborts. */
function untilAborted<T>(promise: Promise<T>, signal: AbortSignal): Promise<T | null> {
	if (signal.aborted) return Promise.resolve(null);
	return new Promise((resolve, reject) => {
		const onAbort = () => resolve(null);
		signal.addEventListener('abort', onAbort, { once: true });
		promise.then(
			(value) => {
				signal.removeEventListener('abort', onAbort);
				resolve(value);
			},
			(err: unknown) => {
				signal.removeEventListener('abort', onAbort);
				reject(err);
			}
		);
	});
}

/**
 * Auto mode's check before a command runs (command-safety.ts): the call's result when it mustn't
 * run, else null. Once it blocked too many, the agent is told to stop, and calls it makes after
 * that are refused unchecked. Stop during the check answers the call like a stopped command.
 */
async function safetyCheck(
	conv: Conversation,
	call: ToolCall,
	input: RunCommandInput,
	dir: string,
	signal: AbortSignal,
	st: State
): Promise<ToolResultBlock | null> {
	// The chat's mode as it is now: someone may have changed it since the turn started.
	if (chatCommandMode(getConversation(conv.id) ?? conv) !== 'auto') return null;
	const tally = st.safety;
	const tooMany = () => tally.streak >= BLOCK_STREAK_LIMIT || tally.total >= BLOCK_TOTAL_LIMIT;
	if (tooMany()) {
		tally.refused++;
		return toolResult(call.id, refusedText(tally.total), true);
	}
	const verdict = await untilAborted(
		checkCommand({
			conv,
			rows: committedRows(conv.id),
			callId: call.id,
			input,
			cwd: resolveCwd(input.cwd, dir),
			profile: { name: getProfile(conv.profileId)?.name ?? '', dir }
		}),
		signal
	);
	if (!verdict) return toolResult(call.id, `Not run. ${stoppedText(st)}`, true);
	if (verdict.allowed) {
		tally.streak = 0;
		return null;
	}
	tally.streak++;
	tally.total++;
	console.log(`[nolune] ${conv.id.slice(0, 8)} auto mode blocked a command: ${verdict.reason}`);
	const stop = tooMany()
		? tally.streak >= BLOCK_STREAK_LIMIT
			? { blocked: tally.streak, inARow: true }
			: { blocked: tally.total, inARow: false }
		: null;
	return toolResult(call.id, blockedText(verdict.reason, stop), true);
}

/**
 * Counts auto mode's blocks from the last message a person wrote: one that came since the count
 * started, maybe to say go ahead, starts it again.
 */
function countBlocksFrom(st: State, rows: MessageRow[]): void {
	const since = rows.findLast((row) => row.kind === 'human')?.seq ?? null;
	if (since !== st.safety.since) st.safety = freshTally(since);
}

function commandSummary(call: ToolCall): string | null {
	const summary = (call.input as { summary?: unknown } | null)?.summary;
	return typeof summary === 'string' && summary.trim() ? summary.trim() : null;
}

/**
 * A background command ended: its output joins the conversation like a message, at the agent's
 * next step or as a new turn. Nothing is handed over for a command someone stopped.
 */
function backgroundCommandEnded(command: BackgroundCommand, result: RunCommandResult): void {
	refreshBackground(command.conversationId);
	if (command.stoppedBy || !getConversation(command.conversationId)) return;
	queueBackgroundResult(command, result.content);
	kick(command.conversationId);
}

function queueBackgroundResult(
	command: Pick<BackgroundCommand, 'conversationId' | 'summary' | 'command'>,
	output: string,
	heading = 'Background command finished'
): void {
	insertQueuedNotice({
		conversationId: command.conversationId,
		kind: 'task_result',
		title: command.summary ?? 'Background command',
		text: output,
		content: `[${heading}${command.summary ? `: ${command.summary}` : ''}]\n$ ${command.command}\n${output}`
	});
	emitQueued(command.conversationId);
}

/**
 * The chat with a system prompt that has its folder and the profile's soul as they are now. When
 * the chat moved to another folder, its folder's instructions or files changed, or the soul
 * changed, the prompt is built again, which costs one prompt cache miss. Only between turns: in
 * the middle of one, the model is still working under the prompt it started with, and its latest
 * thinking must go back with the tool results.
 */
export function withCurrentContext(conv: Conversation, rows: MessageRow[]): Conversation {
	const lastReply = rows.findLast((row) => row.role === 'assistant');
	if (lastReply && rowCalls(lastReply).length) return conv;
	const owner = getProfile(conv.profileId);
	if (!owner) return conv;
	const context = folderContextFor(owner, conv.folderId);
	const soul = readSoul(owner.slug);
	if (context === conv.folderContext && soul.text === conv.soul) return conv;
	const what = [context !== conv.folderContext && 'folder', soul.text !== conv.soul && 'soul']
		.filter(Boolean)
		.join(' and ');
	console.log(`[nolune] ${conv.id.slice(0, 8)} ${what} changed, system prompt built again`);
	return rebuildSystemPrompt(conv, owner, context, soul, lastReply?.seq ?? null);
}

/**
 * Saves a model call's reply in the live reply's place. Pictures and files it links to are copied
 * first (the live reply stays on screen meanwhile), so the saved reply never points at a missing
 * copy. Web pictures only if their link appeared in what the model read before this reply
 * (`found`).
 */
async function saveReply(
	conv: Conversation,
	reply: ModelReply,
	signal: AbortSignal,
	found: () => string
): Promise<void> {
	const conversationId = conv.id;
	const st = stateFor(conversationId);
	const slug = profileSlug(conv.profileId);
	const media: PreparedMedia[] = slug
		? await copyReplyMedia(reply.texts, profileDir(slug), signal, found)
		: [];

	const { usage } = reply;
	console.log(
		usage
			? `[nolune] ${conversationId.slice(0, 8)} ${conv.model} in=${usage.input} cache_read=${usage.cacheRead} cache_write=${usage.cacheWrite} hit=${Math.floor(cacheHitRate(usage) * 100)}% out=${usage.output} stop=${reply.stopReason}`
			: `[nolune] ${conversationId.slice(0, 8)} ${conv.model} stop=${reply.stopReason}`
	);
	const assistantRow = appendRow({
		conversationId,
		role: 'assistant',
		kind: 'assistant',
		content: JSON.stringify(reply.content),
		provider: conv.provider,
		model: conv.model,
		stopReason: reply.stopReason,
		usage,
		media
	});
	// The saved reply takes the live one's place in a single event. As a clear and then the
	// message, the chat could draw in between without the reply, and a reader following it at the
	// bottom was left at its start.
	st.live = [];
	st.toolOutput = null;
	emit(conversationId, {
		type: 'message',
		message: toDisplay(assistantRow, media.length ? listMedia(assistantRow.id) : []),
		replacesLive: true
	});
	touchConversation(conversationId);
}

/**
 * One row with the result of every call of the reply before it, in order. Their `nolune view`
 * pictures were prepared for the conversation's provider; the chat's copies are saved with it.
 */
function saveResults(conv: Conversation, results: ToolResultBlock[]): void {
	const media = results.flatMap((result) => viewedMedia.get(result) ?? []);
	const resultsRow = appendRow({
		conversationId: conv.id,
		role: 'user',
		kind: 'tool_results',
		blocks: results,
		provider: conv.provider,
		media
	});
	emit(conv.id, {
		type: 'message',
		message: toDisplay(resultsRow, media.length ? listMedia(resultsRow.id) : [])
	});
}

/** Rows a chat on a plan sends as the model's input: messages, not command results. */
function isPlanInput(row: MessageRow): boolean {
	return row.role === 'user' && row.kind !== 'tool_results';
}

/**
 * Whether another model answered in the chat since its plan session was last sent anything: the
 * chat switched away from the plan and back. That session never saw those replies, so the chat
 * starts a new one.
 */
function missedReplies(session: { sentSeq: number }, rows: MessageRow[], plan: AgentPlan): boolean {
	return rows.some(
		(row) =>
			(row.seq ?? 0) > session.sentSeq &&
			row.role === 'assistant' &&
			row.provider !== null &&
			row.provider !== plan
	);
}

/**
 * What a chat on a plan sends the plan's agent next, and the session it goes to. Its session
 * keeps the conversation, so only rows it hasn't been sent go. A chat the agent hasn't seen yet
 * may already have replies (a notification opened as a chat, or replies from another model the
 * chat used before): those go along as a transcript, in a new session.
 */
function planInput(
	conv: Conversation & { provider: AgentPlan },
	rows: MessageRow[],
	newSessionId = conv.id
) {
	const sentSeq = rows.at(-1)?.seq ?? 0;
	const content = (row: MessageRow) => readRow(row).blocks;
	const kept = conv.providerSession;
	// Codex's, from when it ran the ChatGPT plan, can't be opened.
	const ours = kept && (kept.provider ?? 'claude-plan') === conv.provider ? kept : null;
	const session = ours && !missedReplies(ours, rows, conv.provider) ? ours : null;
	// The chat's first session has its id.
	if (kept && !session && newSessionId === conv.id) newSessionId = randomUUID();
	let input: Block[];
	if (session) {
		input = rows
			.filter((row) => (row.seq ?? 0) > session.sentSeq && isPlanInput(row))
			.flatMap(content);
	} else {
		// Up to the last reply, or the results of its commands when another model's turn was still
		// going when the chat switched to the plan.
		const seen = rows.findLastIndex((row) => !isPlanInput(row)) + 1;
		const earlier = rows
			.slice(0, seen)
			.map((row) => (row.role === 'assistant' ? `You: ${plainText(row)}` : plainText(row)))
			.filter((text) => text && text !== 'You: ');
		input = rows.slice(seen).flatMap(content);
		if (earlier.length) {
			input.unshift({
				type: 'text',
				text: `[This chat started before you could see it. What was said so far, oldest first:]\n\n${earlier.join('\n\n')}`
			});
		}
	}
	// Nothing new: a turn that failed after the agent took its input, continued.
	if (!input.length) input = [{ type: 'text', text: '[Continue.]' }];
	return {
		sessionId: session?.id ?? newSessionId,
		resume: !!session,
		sentSeq,
		input
	};
}

/**
 * A turn of a chat on a plan, which the plan's agent runs (plans.ts). nolune saves each reply and
 * runs each command as its own loop would; messages sent meanwhile wait for the next turn. False
 * when the loop should end: stopped, or failed with `st.error`.
 */
async function planTurn(
	conv: Conversation & { provider: AgentPlan },
	rows: MessageRow[],
	st: State,
	abort: AbortController
): Promise<boolean> {
	const conversationId = conv.id;
	const slug = profileSlug(conv.profileId);
	if (!slug) {
		st.error = 'The profile no longer exists.';
		return false;
	}
	const cwd = profileDir(slug);
	mkdirSync(cwd, { recursive: true });
	// The plan's agent gets pictures and PDFs inline, so their bytes count too.
	const images = imageUse(rows.map(readRow), true);
	let next = planInput(conv, rows);
	for (let retried = false; ; retried = true) {
		const { sessionId, resume, sentSeq, input } = next;
		try {
			await runPlanTurn(conv.provider, {
				sessionId,
				resume,
				cwd,
				model: conv.model,
				effort: conv.effort,
				system: conv.systemPrompt,
				tools: toolsFor(conv),
				input,
				resolve: async (blocks) =>
					(await resolveFiles([{ role: 'user', blocks }], conv.provider))[0].blocks,
				signal: abort.signal,
				onStarted: (id) =>
					setProviderSession(conversationId, { id, sentSeq, provider: conv.provider }),
				onEvent: (event) => onStreamEvent(conversationId, event),
				onReply: (reply) =>
					saveReply(conv, reply, abort.signal, () => foundText(committedRows(conversationId))),
				// A call that throws still gets its result, or the agent would wait for one forever.
				runTool: (call) =>
					runToolCall(conv, call, 'tool_use', abort.signal, st, images).catch((err: unknown) => {
						st.toolOutput = null;
						console.error(`[nolune] ${conversationId.slice(0, 8)} command failed:`, err);
						const reason = err instanceof Error ? err.message : String(err);
						return toolResult(call.id, `Not finished: ${reason}`, true);
					}),
				onResults: (results) => {
					saveResults(conv, results);
					// The agent went on calling commands after auto mode told it to stop.
					if (st.safety.refused && !abort.signal.aborted) {
						st.error = SAFETY_STOP;
						abort.abort();
					}
				}
			});
			return true;
		} catch (err) {
			clearLive(conversationId);
			if (abort.signal.aborted || isAbortError(err)) {
				// Waiting messages join the transcript unanswered, as after a stop in nolune's own loop.
				commitQueued(conversationId);
				return false;
			}
			const problem = planSessionProblem(conv.provider, err);
			if (problem && !retried) {
				console.error(
					`[nolune] ${conversationId.slice(0, 8)} ${conv.provider} session ${problem}, trying again`
				);
				next =
					problem === 'taken'
						? { ...next, resume: true }
						: planInput({ ...conv, providerSession: null }, rows, randomUUID());
				continue;
			}
			st.error = describeApiError(err);
			console.error(`[nolune] ${conversationId.slice(0, 8)} ${conv.provider} turn failed:`, err);
			return false;
		}
	}
}

async function loop(conversationId: string): Promise<void> {
	const st = stateFor(conversationId);
	if (st.running) return; // the running loop picks up new messages at its next step
	st.running = true;
	st.error = null;
	st.stoppedBy = null;
	st.safety = freshTally();
	emit(conversationId, { type: 'status', running: true, error: null });
	runningChanged(conversationId, true);

	try {
		for (;;) {
			const stored = getConversation(conversationId);
			if (!stored) return;
			commitQueued(conversationId);
			const rows = committedRows(conversationId);
			if (rows.at(-1)?.role !== 'user') return;
			countBlocksFrom(st, rows);
			// The agent went on calling commands after auto mode told it to stop, and nobody has
			// said anything since.
			if (st.safety.refused) {
				st.error = SAFETY_STOP;
				return;
			}
			const conv = withCurrentContext(stored, rows);

			const abort = new AbortController();
			st.abort = abort;
			if (isAgentPlan(conv.provider)) {
				if (!(await planTurn({ ...conv, provider: conv.provider }, rows, st, abort))) return;
				// The agent ended its turn: only messages that came meanwhile start another.
				if (!queuedRows(conversationId).length) return;
				continue;
			}
			const messages = requestMessages(rows, conv.promptChangedAtSeq);
			let reply: ModelReply;
			try {
				reply = await streamTurn({
					provider: conv.provider,
					model: conv.model,
					effort: conv.effort,
					system: conv.systemPrompt,
					tools: toolsFor(conv),
					cacheTtl: conv.cacheTtl,
					cacheKey: conv.id,
					// Pictures and PDFs kept by reference, as this provider gets them, where the model
					// takes them.
					messages: await resolveFiles(
						await readableMessages(conv.provider, conv.model, messages),
						conv.provider
					),
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
				console.error(`[nolune] ${conversationId.slice(0, 8)} model call failed:`, err);
				return;
			}

			await saveReply(conv, reply, abort.signal, () => foundText(rows));

			const { calls } = reply;
			// No tool calls: the turn is over. Loop again in case messages arrived meanwhile.
			if (calls.length === 0) continue;

			// Queued messages may carry pictures too; they join the history at the next step. Without
			// a Files API (the ChatGPT plan), every request carries them inline.
			const images = imageUse(
				[...messages, ...queuedRows(conversationId).map(readRow)],
				!hasFileStore(conv.provider)
			);
			const results: ToolResultBlock[] = [];
			for (const call of calls) {
				// A call that throws still gets its result, or the reply would wait for one forever.
				const result = await runToolCall(
					conv,
					call,
					reply.stopReason,
					abort.signal,
					st,
					images
				).catch((err: unknown) => {
					st.toolOutput = null;
					console.error(`[nolune] ${conversationId.slice(0, 8)} command failed:`, err);
					const reason = err instanceof Error ? err.message : String(err);
					return toolResult(call.id, `Not finished: ${reason}`, true);
				});
				results.push(result);
			}
			saveResults(conv, results);
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
		runningChanged(conversationId, false);
		for (const listener of loopEndListeners) {
			try {
				listener(conversationId, st.error);
			} catch (err) {
				console.error(`[nolune] loop-end listener failed for ${conversationId}:`, err);
			}
		}
	}
}

/**
 * After a restart: answer tool calls that never got a result (appending, never editing), then
 * resume conversations that have queued messages.
 */
export function recoverAfterRestart(): void {
	// Their output is lost; the agent hears that they were cut off, like an unfinished command.
	// The loop below starts their conversations, which now have a queued message.
	for (const row of takeInterruptedBackgroundCommands()) {
		if (!getConversation(row.conversationId)) continue;
		queueBackgroundResult(
			row,
			'Not finished: the gateway restarted while it was running.',
			'Background command cut off'
		);
	}
	for (const id of listAllConversationIds()) {
		// `pnpm dev` runs this again when a file changes, without a restart: a loop still running
		// here answers its own calls, and a second answer would break the conversation.
		if (isRunning(id)) continue;
		const last = lastCommittedRow(id);
		if (last?.kind === 'assistant') {
			const calls = rowCalls(last);
			if (calls.length) {
				appendRow({
					conversationId: id,
					role: 'user',
					kind: 'tool_results',
					blocks: calls.map((c) =>
						toolResult(c.id, 'Not finished: the gateway restarted while this was running.', true)
					)
				});
			}
		}
		// The subagent host starts subagents again itself, once it's writing their log.
		if (queuedRows(id).length && !isSubagentConversation(id)) kick(id);
	}
}
