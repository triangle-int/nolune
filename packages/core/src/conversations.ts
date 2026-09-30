import { randomUUID } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import { and, desc, eq, gte, inArray, isNotNull, isNull, lt, max } from 'drizzle-orm';
import { parseAttachments, type MessageAttachment } from './attachments.ts';
import { getDb } from './db/index.ts';
import {
	conversation,
	media,
	message,
	profile,
	profileMember,
	subagent,
	triggerRun,
	upload
} from './db/schema.ts';
import { folderContextFor, getFolder } from './folders.ts';
import {
	messageText,
	placeholder,
	readMessage,
	resultText,
	type Block,
	type FileProvider,
	type Message,
	type ToolCallBlock,
	type ToolResultBlock
} from './format.ts';
import {
	newMediaId,
	toDisplayMedia,
	viewedIndex,
	type DisplayMedia,
	type MediaRow,
	type PreparedMedia
} from './media.ts';
import type { CommandMode } from './command-safety.ts';
import type { CacheTtl, Effort, Provider } from './models.ts';
import { buildSystemPrompt } from './prompt.ts';
import { effectiveContextWindow, getPreset } from './presets.ts';
import type { Profile } from './profiles.ts';
import { LEGACY_TOOLS, TOOLS } from './run-command.ts';
import { readSoul } from './soul.ts';
import { promptTokens } from './usage.ts';

export type Conversation = typeof conversation.$inferSelect;
export type MessageRow = typeof message.$inferSelect;

export interface Usage {
	input: number;
	cacheRead: number;
	cacheWrite: number;
	output: number;
}

export type DisplayBlock =
	| { type: 'text'; text: string }
	| { type: 'thinking'; text: string }
	| {
			type: 'tool';
			id: string;
			command: string;
			cwd?: string;
			/** Plain-language description and Lucide icon name the model wrote with the call. */
			summary?: string;
			icon?: string;
	  };

/** A file someone attached, as the chat shows it. */
export type DisplayAttachment = Extract<DisplayMedia, { status: 'ok' }> & {
	sentAs: MessageAttachment['sentAs'];
	/** Why the model got only its name and path. */
	note?: string;
};

/** A picture a command attached with `nolune view`, as the chat shows it under the command. */
export type DisplayPicture = Extract<DisplayMedia, { status: 'ok' }>;

/** A command's result, as the chat shows it. */
export interface DisplayResult {
	id: string;
	output: string;
	isError: boolean;
	/** What it attached with `nolune view`, in order. `output` leaves them out. */
	pictures: DisplayPicture[];
}

export type DisplayMessage =
	| {
			id: number;
			kind: 'human';
			/** Null once they're deleted. */
			senderId: string | null;
			senderName: string;
			text: string;
			attachments: DisplayAttachment[];
			queued: boolean;
			createdAt: number;
	  }
	| {
			id: number;
			kind: 'trigger';
			/** The trigger's name. */
			title: string;
			text: string;
			createdAt: number;
	  }
	| {
			id: number;
			/** In a subagent's chat: its task, or a steer, from the agent that started it. */
			kind: 'agent_message';
			/** The subagent's id. */
			title: string;
			text: string;
			createdAt: number;
	  }
	| {
			id: number;
			/** What a background command printed, handed to the agent when it ended. */
			kind: 'task_result';
			/** The command's summary. */
			title: string;
			output: string;
			isError: boolean;
			createdAt: number;
	  }
	| {
			id: number;
			kind: 'assistant';
			blocks: DisplayBlock[];
			/** The pictures and files its text links to, keyed by link target. */
			media: Record<string, DisplayMedia>;
			stopReason: string | null;
			usage: Usage | null;
			/** The model that wrote it: a conversation can switch models. */
			model: string | null;
			createdAt: number;
	  }
	| {
			id: number;
			kind: 'tool_results';
			results: DisplayResult[];
			createdAt: number;
	  };

/** The model a conversation is created with: a preset's, or another conversation's. */
type ModelChoice = { presetId: string } | { modelOf: Conversation };

function modelColumns(choice: ModelChoice) {
	if ('modelOf' in choice) {
		const { presetId, presetName, provider, model, contextWindow } = choice.modelOf;
		return { presetId, presetName, provider, model, contextWindow };
	}
	const preset = getPreset(choice.presetId);
	if (!preset) throw new Error('Unknown model preset');
	return {
		presetId: preset.id,
		presetName: preset.name,
		provider: preset.provider,
		model: preset.model,
		contextWindow: effectiveContextWindow(preset)
	};
}

export function createConversation(
	input: ModelChoice & {
		profile: Profile;
		/** Null for background runs and subagents, which no person started. */
		userId: string | null;
		effort?: Effort;
		title?: string;
		hidden?: boolean;
		/** A folder of the profile: the chat starts in it, with its instructions and files. */
		folderId?: string | null;
		cacheTtl?: CacheTtl;
		/** How its commands run, when that differs from Models & keys (command-safety.ts). */
		commandMode?: CommandMode | null;
	}
): Conversation {
	const folderId = input.folderId ?? null;
	if (folderId && !getFolder(input.profile.id, folderId)) throw new Error('Unknown folder');
	const folderContext = folderContextFor(input.profile, folderId);
	const soul = readSoul(input.profile.slug);
	const now = new Date();
	const created: Conversation = {
		id: randomUUID(),
		profileId: input.profile.id,
		title: input.title ?? '',
		...modelColumns(input),
		effort: input.effort ?? 'medium',
		systemPrompt: buildSystemPrompt(input.profile, folderContext, soul),
		folderId,
		folderContext,
		soul: soul.text,
		promptChangedAtSeq: null,
		tools: TOOLS,
		providerSession: null,
		cacheTtl: input.cacheTtl ?? '1h',
		commandMode: input.commandMode ?? null,
		hidden: input.hidden ?? false,
		learnedSeq: null,
		createdBy: input.userId,
		createdAt: now,
		updatedAt: now
	};
	getDb().insert(conversation).values(created).run();
	return created;
}

export function listConversations(profileId: string) {
	return getDb()
		.select({
			id: conversation.id,
			title: conversation.title,
			presetName: conversation.presetName,
			folderId: conversation.folderId,
			updatedAt: conversation.updatedAt
		})
		.from(conversation)
		.where(and(eq(conversation.profileId, profileId), eq(conversation.hidden, false)))
		.orderBy(desc(conversation.updatedAt))
		.all();
}

export function getConversation(id: string): Conversation | undefined {
	return getDb().select().from(conversation).where(eq(conversation.id, id)).get();
}

/** The tool definitions the conversation's requests send, as they were when it was created. */
export function toolsFor(conv: Pick<Conversation, 'tools'>): Anthropic.Tool[] {
	return conv.tools ?? LEGACY_TOOLS;
}

/** True for a subagent's own conversation, which only the agent that started it writes to. */
export function isSubagentConversation(id: string): boolean {
	return !!getDb()
		.select({ id: subagent.id })
		.from(subagent)
		.where(eq(subagent.conversationId, id))
		.get();
}

/** Conversations in the list (not background runs or subagents) that were active since `since`. */
export function recentConversationIds(since: Date): string[] {
	return getDb()
		.select({ id: conversation.id })
		.from(conversation)
		.where(and(eq(conversation.hidden, false), gte(conversation.updatedAt, since)))
		.all()
		.map((r) => r.id);
}

export function listAllConversationIds(): string[] {
	return getDb()
		.select({ id: conversation.id })
		.from(conversation)
		.all()
		.map((r) => r.id);
}

/** The conversation and its profile, if the user is a member of that profile. */
export function getConversationForUser(id: string, userId: string) {
	return getDb()
		.select({ conversation, profile })
		.from(conversation)
		.innerJoin(profile, eq(profile.id, conversation.profileId))
		.innerJoin(
			profileMember,
			and(eq(profileMember.profileId, profile.id), eq(profileMember.userId, userId))
		)
		.where(eq(conversation.id, id))
		.get();
}

/** On Claude, changing effort mid-conversation rebuilds that conversation's cache once. */
export function setEffort(id: string, effort: Effort): void {
	getDb().update(conversation).set({ effort }).where(eq(conversation.id, id)).run();
}

/** Null: the chat's commands run as Models & keys says. */
export function setCommandMode(id: string, commandMode: CommandMode | null): void {
	getDb().update(conversation).set({ commandMode }).where(eq(conversation.id, id)).run();
}

/** Why a conversation can't switch to a model, in words for the person switching. */
export class ModelSwitchError extends Error {}

/** The prompt and reply of the conversation's latest model call, in tokens, or 0 before one. */
function contextUsed(conversationId: string): number {
	const last = getDb()
		.select({ usage: message.usage })
		.from(message)
		.where(
			and(
				eq(message.conversationId, conversationId),
				eq(message.role, 'assistant'),
				isNotNull(message.usage)
			)
		)
		.orderBy(desc(message.seq))
		.limit(1)
		.get();
	if (!last?.usage) return 0;
	const usage = JSON.parse(last.usage) as Usage;
	return promptTokens(usage) + usage.output;
}

/**
 * Switches the conversation to another model preset, from its next model call on (a turn in
 * progress goes on with the new model too). The new model has none of the conversation cached, so
 * that call reads it all again once. Another provider gets earlier replies and files translated
 * (requestMessages). Refused when the conversation is already larger than the new model's window,
 * which it could never shrink back into: history is never edited.
 */
export function setPreset(id: string, presetId: string): Conversation {
	const conv = getConversation(id);
	if (!conv) throw new ModelSwitchError('This chat no longer exists.');
	const preset = getPreset(presetId);
	if (!preset) throw new ModelSwitchError('That model was removed. Pick another one.');
	const columns = modelColumns({ presetId: preset.id });
	const used = contextUsed(id);
	if (columns.contextWindow !== null && used > columns.contextWindow) {
		throw new ModelSwitchError(
			`This chat is already about ${used.toLocaleString('en-US')} tokens, more than ${preset.name} can read (${columns.contextWindow.toLocaleString('en-US')}). Start a new chat for it.`
		);
	}
	getDb().update(conversation).set(columns).where(eq(conversation.id, id)).run();
	return { ...conv, ...columns };
}

/**
 * Builds the system prompt again, with the chat's folder (`folderContext`) and the profile's soul
 * as they are now, and notes the last row before it: its thinking was made under the old prompt
 * (requestMessages). The whole prompt is rebuilt, so the skills catalog and memory are current
 * again too.
 */
export function rebuildSystemPrompt(
	conv: Conversation,
	profile: Pick<Profile, 'id' | 'slug' | 'disabledSkills'>,
	folderContext: string,
	soul: { text: string; cut: boolean },
	lastSeq: number | null
): Conversation {
	const changed = {
		systemPrompt: buildSystemPrompt(profile, folderContext, soul),
		folderContext,
		soul: soul.text,
		promptChangedAtSeq: lastSeq ?? conv.promptChangedAtSeq
	};
	getDb().update(conversation).set(changed).where(eq(conversation.id, conv.id)).run();
	return { ...conv, ...changed };
}

/** Chats on a plan: its agent's session, and the last row it was sent. */
export function setProviderSession(id: string, session: Conversation['providerSession']): void {
	getDb()
		.update(conversation)
		.set({ providerSession: session })
		.where(eq(conversation.id, id))
		.run();
}

/** A background run becomes a normal conversation once someone continues it. */
export function setHidden(id: string, hidden: boolean): void {
	getDb().update(conversation).set({ hidden }).where(eq(conversation.id, id)).run();
}

/** nolune has looked over the conversation for memory up to row `seq`. */
export function setLearnedSeq(id: string, seq: number): void {
	getDb().update(conversation).set({ learnedSeq: seq }).where(eq(conversation.id, id)).run();
}

/** Background runs nobody continued, and subagents, last active before `before`. */
export function deleteHiddenConversations(before: Date): void {
	getDb()
		.delete(conversation)
		.where(and(eq(conversation.hidden, true), lt(conversation.updatedAt, before)))
		.run();
}

export function deleteConversation(id: string): void {
	getDb().transaction((tx) => {
		// A background run deleted mid-way never reaches its end, so it's settled here.
		tx.update(triggerRun)
			.set({ status: 'stopped', finishedAt: new Date() })
			.where(and(eq(triggerRun.conversationId, id), eq(triggerRun.status, 'running')))
			.run();
		// Its subagents' conversations go with it.
		const children = tx
			.select({ id: subagent.conversationId })
			.from(subagent)
			.where(eq(subagent.parentId, id))
			.all()
			.map((row) => row.id);
		if (children.length) tx.delete(conversation).where(inArray(conversation.id, children)).run();
		tx.delete(conversation).where(eq(conversation.id, id)).run();
	});
}

export function touchConversation(id: string, title?: string): void {
	getDb()
		.update(conversation)
		.set({ updatedAt: new Date(), ...(title !== undefined ? { title } : {}) })
		.where(eq(conversation.id, id))
		.run();
}

/** Unlike touchConversation, leaves the chat where it is in the list. */
export function setTitle(id: string, title: string): void {
	getDb().update(conversation).set({ title }).where(eq(conversation.id, id)).run();
}

/** Sets the title unless it changed since it read `from`. True if it was set. */
export function replaceTitle(id: string, from: string, to: string): boolean {
	const result = getDb()
		.update(conversation)
		.set({ title: to })
		.where(and(eq(conversation.id, id), eq(conversation.title, from)))
		.run();
	return result.changes > 0;
}

export function committedRows(conversationId: string): MessageRow[] {
	return getDb()
		.select()
		.from(message)
		.where(and(eq(message.conversationId, conversationId), isNotNull(message.seq)))
		.orderBy(message.seq)
		.all();
}

export function queuedRows(conversationId: string): MessageRow[] {
	return getDb()
		.select()
		.from(message)
		.where(and(eq(message.conversationId, conversationId), isNull(message.seq)))
		.orderBy(message.id)
		.all();
}

export function lastCommittedRow(conversationId: string): MessageRow | undefined {
	return getDb()
		.select()
		.from(message)
		.where(and(eq(message.conversationId, conversationId), isNotNull(message.seq)))
		.orderBy(desc(message.seq))
		.limit(1)
		.get();
}

function nextSeq(conversationId: string): number {
	const row = getDb()
		.select({ seq: max(message.seq) })
		.from(message)
		.where(eq(message.conversationId, conversationId))
		.get();
	return (row?.seq ?? 0) + 1;
}

export function insertQueued(input: {
	conversationId: string;
	senderId: string;
	senderName: string;
	text: string;
	/** The provider its pictures and PDFs were prepared for: the conversation's. */
	provider?: Provider;
	/** With attachments (see prepareMessage): the content, the files, and the uploads they were. */
	attachments?: {
		content: Block[];
		files: MessageAttachment[];
		media: (PreparedMedia & { id: string })[];
		uploadIds: string[];
	};
	/** What memory has on the message (recallFor), for the model only: the chat shows `text`. */
	recall?: string | null;
}): MessageRow {
	const { attachments } = input;
	// Without attachments the model sees only the sender's name and what they wrote.
	const content: Block[] = [
		...(attachments?.content ?? [{ type: 'text', text: `${input.senderName}: ${input.text}` }]),
		...(input.recall ? [{ type: 'text' as const, text: input.recall }] : [])
	];
	return getDb().transaction((tx) => {
		if (attachments?.uploadIds.length) {
			const taken = tx
				.delete(upload)
				.where(inArray(upload.id, attachments.uploadIds))
				.returning({ id: upload.id })
				.all();
			if (taken.length !== attachments.uploadIds.length) {
				throw new Error('An attached file was already sent.');
			}
		}
		const row = tx
			.insert(message)
			.values({
				conversationId: input.conversationId,
				seq: null,
				role: 'user',
				kind: 'human',
				senderId: input.senderId,
				senderName: input.senderName,
				text: input.text,
				content: JSON.stringify(content),
				format: 'nolune',
				attachments: attachments ? JSON.stringify(attachments.files) : null,
				provider: input.provider ?? null,
				createdAt: new Date()
			})
			.returning()
			.get();
		if (attachments?.media.length) {
			tx.insert(media)
				.values(
					attachments.media.map((m) => ({
						...m,
						conversationId: input.conversationId,
						messageId: row.id
					}))
				)
				.run();
		}
		return row;
	});
}

/**
 * Queues a message the gateway writes, not a person: a subagent's task or steer, or a background
 * command's output. Like a person's message, it joins the transcript at the agent's next step, or
 * starts a turn.
 */
export function insertQueuedNotice(input: {
	conversationId: string;
	kind: 'agent_message' | 'task_result';
	/** For display: the subagent's id, or the command's summary. */
	title: string;
	/** For display: what the agent wrote, or what the command printed. */
	text: string;
	/** What the model reads. */
	content: string;
}): MessageRow {
	return getDb()
		.insert(message)
		.values({
			conversationId: input.conversationId,
			seq: null,
			role: 'user',
			kind: input.kind,
			senderName: input.title,
			text: input.text,
			content: JSON.stringify([{ type: 'text', text: input.content }] satisfies Block[]),
			format: 'nolune',
			createdAt: new Date()
		})
		.returning()
		.get();
}

/** Moves queued messages into the transcript, in the order they were sent. */
export function commitQueuedRows(conversationId: string): MessageRow[] {
	return getDb().transaction((tx) => {
		const queued = tx
			.select()
			.from(message)
			.where(and(eq(message.conversationId, conversationId), isNull(message.seq)))
			.orderBy(message.id)
			.all();
		if (queued.length === 0) return [];
		let seq = nextSeq(conversationId);
		return queued.map((row) =>
			tx.update(message).set({ seq: seq++ }).where(eq(message.id, row.id)).returning().get()
		);
	});
}

export function appendRow(
	input: {
		conversationId: string;
		role: 'user' | 'assistant';
		kind: 'trigger' | 'tool_results' | 'assistant';
		/** Trigger rows: the trigger's name and prompt, for display. */
		senderName?: string;
		text?: string;
		stopReason?: string | null;
		usage?: Usage | null;
		/**
		 * Replies: the provider and model that wrote it. Command results: the provider their
		 * pictures were prepared for.
		 */
		provider?: Provider | null;
		model?: string | null;
		/**
		 * Copies saved with the row: the pictures and files a reply links to, the pictures commands
		 * attached with `nolune view`.
		 */
		media?: PreparedMedia[];
	} & (
		| {
				/** nolune's own content, stored in nolune's format. */
				blocks: Block[];
		  }
		| {
				/** A reply as its provider returned it, as JSON: stored as it is. */
				content: string;
		  }
	)
): MessageRow {
	return getDb().transaction((tx) => {
		const row = tx
			.insert(message)
			.values({
				conversationId: input.conversationId,
				seq: nextSeq(input.conversationId),
				role: input.role,
				kind: input.kind,
				senderName: input.senderName ?? null,
				text: input.text ?? null,
				...('blocks' in input
					? { content: JSON.stringify(input.blocks), format: 'nolune' as const }
					: { content: input.content }),
				provider: input.provider ?? null,
				model: input.model ?? null,
				stopReason: input.stopReason ?? null,
				usage: input.usage ? JSON.stringify(input.usage) : null,
				createdAt: new Date()
			})
			.returning()
			.get();
		if (input.media?.length) {
			tx.insert(media)
				.values(
					input.media.map((m) => ({
						...m,
						id: newMediaId(),
						conversationId: input.conversationId,
						messageId: row.id
					}))
				)
				.run();
		}
		return row;
	});
}

/** A row in nolune's format (format.ts), whichever way it was stored. */
export function readRow(row: MessageRow): Message {
	return readMessage(row);
}

/**
 * The providers holding pictures and PDFs of the conversation that no other provider can open:
 * those sent before nolune kept its own copies, as a Files API id. Newer ones go to any provider.
 */
export function heldFileProviders(conversationId: string): FileProvider[] {
	const held = new Set<FileProvider>();
	const add = (b: Block) => {
		if ((b.type === 'image' || b.type === 'pdf') && b.source.type === 'uploaded') {
			if (b.source.provider) held.add(b.source.provider);
		}
	};
	for (const row of committedRows(conversationId)) {
		if (row.role !== 'user') continue;
		for (const b of readRow(row).blocks) {
			add(b);
			if (b.type === 'tool_result' && Array.isArray(b.content)) b.content.forEach(add);
		}
	}
	return [...held];
}

/** The tool calls of a reply row. */
export function rowCalls(row: MessageRow): ToolCallBlock[] {
	return readRow(row).blocks.filter((b) => b.type === 'tool_call');
}

/**
 * The transcript for a model call, in nolune's format: every row, with its replies marked when
 * they're from before the system prompt was last built again (`promptChangedAtSeq`), and every
 * tool call answered once (pairToolResults). Each provider's module turns it into its request
 * (format.ts), and does it the same way on every call, so the prefix stays byte-identical.
 */
export function requestMessages(rows: MessageRow[], promptChangedAtSeq: number | null): Message[] {
	const messages = rows.map((row): Message => {
		const read = readRow(row);
		const before =
			row.role === 'assistant' &&
			promptChangedAtSeq !== null &&
			row.seq !== null &&
			row.seq <= promptChangedAtSeq;
		return before ? { ...read, beforePromptChange: true } : read;
	});
	return pairToolResults(messages);
}

function noResult(callId: string): ToolResultBlock {
	return {
		type: 'tool_result',
		callId,
		content: 'No result came back from this command. It may or may not have run.',
		isError: true
	};
}

/**
 * The API refuses the whole transcript unless every tool call has exactly one result, right
 * after the reply that made it and before anything else. A crash or restart in the middle of a
 * command could break that for good: a call left without a result, or one answered twice (the
 * dev server's reload answered a call that was still running). So a call without a result gets
 * one saying so, and a result that's a second one, late, or for no call is left out. Messages
 * that need no mending are passed through as they are, so healthy transcripts don't change.
 */
function pairToolResults(messages: Message[]): Message[] {
	const out: Message[] = [];
	/** Calls of the latest reply that have no result yet. */
	let open: string[] = [];
	for (const m of messages) {
		if (m.role === 'assistant') {
			if (open.length) out.push({ role: 'user', blocks: open.map(noResult) });
			open = m.blocks.flatMap((b) => (b.type === 'tool_call' ? [b.id] : []));
			out.push(m);
			continue;
		}
		const content: Block[] = [];
		for (const b of m.blocks) {
			if (b.type !== 'tool_result') {
				content.push(...open.map(noResult), b);
				open = [];
			} else if (open.includes(b.callId)) {
				content.push(b);
				open = open.filter((id) => id !== b.callId);
			}
		}
		if (content.length === m.blocks.length && content.every((b, i) => b === m.blocks[i])) {
			out.push(m);
		} else if (content.length) out.push({ role: 'user', blocks: content });
	}
	if (open.length && out.at(-1)?.role === 'user') {
		out.push({ role: 'user', blocks: open.map(noResult) });
	}
	return out;
}

/**
 * A row as plain text: what was said, and what commands printed. Pictures, files and the agent's
 * commands themselves are left out.
 */
export function plainText(row: MessageRow): string {
	if (row.role === 'assistant') return replyText(row);
	return messageText(readRow(row).blocks);
}

/** The text blocks of an assistant row: what the agent said, without thinking or commands. */
export function replyText(row: MessageRow): string {
	return readRow(row)
		.blocks.flatMap((b) => (b.type === 'text' ? [b.text] : []))
		.join('\n\n')
		.trim();
}

/**
 * Everything the model read that it didn't write: what people wrote, automation prompts and
 * events, and command output. A web picture in a reply is downloaded only if its link is in here.
 * A subagent's task and steers are left out: another agent wrote them.
 */
export function foundText(rows: MessageRow[]): string {
	return rows
		.filter((row) => row.kind !== 'assistant' && row.kind !== 'agent_message')
		.flatMap((row) =>
			readRow(row).blocks.map((b) =>
				b.type === 'tool_result' ? resultText(b.content) : b.type === 'text' ? b.text : ''
			)
		)
		.join('\n');
}

function displayAttachments(row: MessageRow, mediaRows: MediaRow[]): DisplayAttachment[] {
	return parseAttachments(row.attachments).flatMap((attachment) => {
		const mediaRow = mediaRows.find((m) => m.id === attachment.mediaId);
		const shown = mediaRow && toDisplayMedia([mediaRow])[mediaRow.src];
		if (shown?.status !== 'ok') return [];
		return [{ ...shown, sentAs: attachment.sentAs, note: attachment.note }];
	});
}

function displayResult(block: ToolResultBlock, mediaRows: MediaRow[]): DisplayResult {
	const pictures = mediaRows
		.flatMap((row) => {
			const index = viewedIndex(row.src, block.callId);
			if (index === null) return [];
			const shown = toDisplayMedia([row])[row.src];
			return shown.status === 'ok' ? [{ index, shown }] : [];
		})
		.sort((a, b) => a.index - b.index)
		.map((p) => p.shown);
	const { content } = block;
	// The chat shows the pictures under the output: not as `[image]`, nor the line naming each.
	const output =
		pictures.length && typeof content !== 'string'
			? content
					.flatMap((b, i) => {
						if (b.type === 'image') return [];
						if (b.type === 'text') return content[i + 1]?.type === 'image' ? [] : [b.text];
						return [`[${placeholder(b)}]`];
					})
					.join('\n')
			: resultText(content);
	return { id: block.callId, output, isError: block.isError, pictures };
}

/**
 * `mediaRows`: the row's pictures and files (a reply's links, a message's attachments, what its
 * commands attached with `nolune view`).
 */
export function toDisplay(row: MessageRow, mediaRows: MediaRow[] = []): DisplayMessage {
	const createdAt = row.createdAt.getTime();
	if (row.kind === 'trigger') {
		return {
			id: row.id,
			kind: 'trigger',
			title: row.senderName ?? 'Automation',
			text: row.text ?? '',
			createdAt
		};
	}
	if (row.kind === 'agent_message') {
		return {
			id: row.id,
			kind: 'agent_message',
			title: row.senderName ?? 'Subagent',
			text: row.text ?? '',
			createdAt
		};
	}
	if (row.kind === 'task_result') {
		const output = row.text ?? '';
		return {
			id: row.id,
			kind: 'task_result',
			title: row.senderName ?? 'Background command',
			output,
			// runCommand ends the output of a command that exited by itself with its exit code.
			isError: !/\[exit code 0\]$/.test(output),
			createdAt
		};
	}
	if (row.kind === 'human') {
		return {
			id: row.id,
			kind: 'human',
			senderId: row.senderId,
			senderName: row.senderName ?? 'Someone',
			text: row.text ?? '',
			attachments: displayAttachments(row, mediaRows),
			queued: row.seq === null,
			createdAt
		};
	}
	if (row.kind === 'tool_results') {
		return {
			id: row.id,
			kind: 'tool_results',
			results: readRow(row).blocks.flatMap((b) =>
				b.type === 'tool_result' ? [displayResult(b, mediaRows)] : []
			),
			createdAt
		};
	}
	const blocks: DisplayBlock[] = [];
	for (const block of readRow(row).blocks) {
		if (block.type === 'text' || block.type === 'reasoning') {
			if (block.text.trim()) {
				blocks.push({ type: block.type === 'text' ? 'text' : 'thinking', text: block.text });
			}
		} else if (block.type === 'tool_call') {
			const input = (block.input ?? {}) as Record<string, unknown>;
			const text = (key: string) =>
				typeof input[key] === 'string' && input[key].trim() ? { [key]: input[key].trim() } : {};
			blocks.push({
				type: 'tool',
				id: block.id,
				command: typeof input.command === 'string' ? input.command : JSON.stringify(block.input),
				...text('cwd'),
				...text('summary'),
				...text('icon')
			});
		}
	}
	return {
		id: row.id,
		kind: 'assistant',
		blocks,
		media: toDisplayMedia(mediaRows),
		stopReason: row.stopReason,
		usage: row.usage ? (JSON.parse(row.usage) as Usage) : null,
		model: row.model,
		createdAt
	};
}
