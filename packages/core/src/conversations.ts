import { randomUUID } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import { and, desc, eq, inArray, isNotNull, isNull, lt, max } from 'drizzle-orm';
import type { Effort } from './anthropic.ts';
import { parseAttachments, type MessageAttachment } from './attachments.ts';
import { getDb } from './db/index.ts';
import {
	conversation,
	media,
	message,
	profile,
	profileMember,
	triggerRun,
	upload
} from './db/schema.ts';
import { folderContextFor, getFolder } from './folders.ts';
import {
	newMediaId,
	toDisplayMedia,
	type DisplayMedia,
	type MediaRow,
	type PreparedMedia
} from './media.ts';
import { buildSystemPrompt } from './prompt.ts';
import { effectiveContextWindow, getPreset } from './presets.ts';
import type { Profile } from './profiles.ts';
import { readSoul } from './soul.ts';

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

export type DisplayMessage =
	| {
			id: number;
			kind: 'human';
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
			kind: 'assistant';
			blocks: DisplayBlock[];
			/** The pictures and files its text links to, keyed by link target. */
			media: Record<string, DisplayMedia>;
			stopReason: string | null;
			usage: Usage | null;
			createdAt: number;
	  }
	| {
			id: number;
			kind: 'tool_results';
			results: { id: string; output: string; isError: boolean }[];
			createdAt: number;
	  };

export function createConversation(input: {
	profile: Profile;
	presetId: string;
	/** Null for background runs, which no person started. */
	userId: string | null;
	effort?: Effort;
	title?: string;
	hidden?: boolean;
	/** A folder of the profile: the chat starts in it, with its instructions and files. */
	folderId?: string | null;
}): Conversation {
	const preset = getPreset(input.presetId);
	if (!preset) throw new Error('Unknown model preset');
	const folderId = input.folderId ?? null;
	if (folderId && !getFolder(input.profile.id, folderId)) throw new Error('Unknown folder');
	const folderContext = folderContextFor(input.profile, folderId);
	const soul = readSoul(input.profile.slug);
	const now = new Date();
	const created: Conversation = {
		id: randomUUID(),
		profileId: input.profile.id,
		title: input.title ?? '',
		presetId: preset.id,
		presetName: preset.name,
		provider: preset.provider,
		model: preset.model,
		contextWindow: effectiveContextWindow(preset),
		effort: input.effort ?? 'medium',
		systemPrompt: buildSystemPrompt(input.profile, folderContext, soul),
		folderId,
		folderContext,
		soul: soul.text,
		promptChangedAtSeq: null,
		hidden: input.hidden ?? false,
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

/**
 * Builds the system prompt again, with the chat's folder (`folderContext`) and the profile's soul
 * as they are now, and notes the last row before it: its thinking was made under the old prompt
 * (requestMessages). The whole prompt is rebuilt, so the skills catalog and memory are current
 * again too.
 */
export function rebuildSystemPrompt(
	conv: Conversation,
	profile: Pick<Profile, 'slug' | 'disabledSkills'>,
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

/** A background run becomes a normal conversation once someone continues it. */
export function setHidden(id: string, hidden: boolean): void {
	getDb().update(conversation).set({ hidden }).where(eq(conversation.id, id)).run();
}

/** Background runs nobody continued, last active before `before`. */
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
	/** With attachments (see prepareMessage): the content, the files, and the uploads they were. */
	attachments?: {
		content: unknown[];
		files: MessageAttachment[];
		media: (PreparedMedia & { id: string })[];
		uploadIds: string[];
	};
}): MessageRow {
	const { attachments } = input;
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
				// Without attachments the model sees only the sender's name and what they wrote.
				content: JSON.stringify(
					attachments?.content ?? [{ type: 'text', text: `${input.senderName}: ${input.text}` }]
				),
				attachments: attachments ? JSON.stringify(attachments.files) : null,
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

export function appendRow(input: {
	conversationId: string;
	role: 'user' | 'assistant';
	kind: 'trigger' | 'tool_results' | 'assistant';
	content: string;
	/** Trigger rows: the trigger's name and prompt, for display. */
	senderName?: string;
	text?: string;
	stopReason?: string | null;
	usage?: Usage | null;
	/** Assistant rows: copies of the pictures and files the reply links to, saved with it. */
	media?: PreparedMedia[];
}): MessageRow {
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
				content: input.content,
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

/** Exactly what was stored, so the request prefix is byte-identical to the previous call. */
export function toMessageParam(row: MessageRow): Anthropic.MessageParam {
	return { role: row.role, content: JSON.parse(row.content) };
}

/**
 * The transcript as a model call sends it: every row exactly as stored, except the thinking in
 * replies from before the system prompt was last built again (`promptChangedAtSeq`). A thinking
 * block's signature records the prompt it was made under, and newer models refuse it under
 * another one, so those are left out. They are always the oldest ones, which the API allows,
 * and they are left out the same way on every call, so the prefix stays byte-identical.
 */
export function requestMessages(
	rows: MessageRow[],
	promptChangedAtSeq: number | null
): Anthropic.MessageParam[] {
	const messages = rows.flatMap((row): Anthropic.MessageParam[] => {
		if (promptChangedAtSeq === null || row.role !== 'assistant' || row.seq === null) {
			return [toMessageParam(row)];
		}
		if (row.seq > promptChangedAtSeq) return [toMessageParam(row)];
		const content = (JSON.parse(row.content) as Anthropic.ContentBlockParam[]).filter(
			(b) => b.type !== 'thinking' && b.type !== 'redacted_thinking'
		);
		// A reply that was only thinking (cut off, say) has nothing left to send.
		return content.length ? [{ role: 'assistant', content }] : [];
	});
	return pairToolResults(messages);
}

function noResult(toolUseId: string): Anthropic.ToolResultBlockParam {
	return {
		type: 'tool_result',
		tool_use_id: toolUseId,
		content: 'No result came back from this command. It may or may not have run.',
		is_error: true
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
function pairToolResults(messages: Anthropic.MessageParam[]): Anthropic.MessageParam[] {
	const out: Anthropic.MessageParam[] = [];
	/** Calls of the latest reply that have no result yet. */
	let open: string[] = [];
	for (const m of messages) {
		if (m.role === 'assistant') {
			if (open.length) out.push({ role: 'user', content: open.map(noResult) });
			open =
				typeof m.content === 'string'
					? []
					: m.content.flatMap((b) => (b.type === 'tool_use' ? [b.id] : []));
			out.push(m);
			continue;
		}
		const blocks: Anthropic.ContentBlockParam[] =
			typeof m.content === 'string' ? [{ type: 'text', text: m.content }] : m.content;
		const content: Anthropic.ContentBlockParam[] = [];
		for (const b of blocks) {
			if (b.type !== 'tool_result') {
				content.push(...open.map(noResult), b);
				open = [];
			} else if (open.includes(b.tool_use_id)) {
				content.push(b);
				open = open.filter((id) => id !== b.tool_use_id);
			}
		}
		if (content.length === blocks.length && content.every((b, i) => b === blocks[i])) out.push(m);
		else if (content.length) out.push({ role: 'user', content });
	}
	if (open.length && out.at(-1)?.role === 'user') {
		out.push({ role: 'user', content: open.map(noResult) });
	}
	return out;
}

export function summarizeUsage(usage: Anthropic.Usage): Usage {
	return {
		input: usage.input_tokens,
		cacheRead: usage.cache_read_input_tokens ?? 0,
		cacheWrite: usage.cache_creation_input_tokens ?? 0,
		output: usage.output_tokens
	};
}

function toolResultText(content: Anthropic.ToolResultBlockParam['content']): string {
	if (typeof content === 'string') return content;
	return (content ?? []).map((b) => (b.type === 'text' ? b.text : `[${b.type}]`)).join('\n');
}

/** The text blocks of an assistant row: what the agent said, without thinking or commands. */
export function replyText(row: MessageRow): string {
	const content = JSON.parse(row.content) as Anthropic.ContentBlock[];
	return content
		.flatMap((b) => (b.type === 'text' ? [b.text] : []))
		.join('\n\n')
		.trim();
}

/**
 * Everything the model read that it didn't write: what people wrote, automation prompts and
 * events, and command output. A web picture in a reply is downloaded only if its link is in here.
 */
export function foundText(rows: MessageRow[]): string {
	return rows
		.filter((row) => row.kind !== 'assistant')
		.flatMap((row) =>
			(
				JSON.parse(row.content) as (Anthropic.TextBlockParam | Anthropic.ToolResultBlockParam)[]
			).map((b) =>
				b.type === 'tool_result' ? toolResultText(b.content) : b.type === 'text' ? b.text : ''
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

/** `mediaRows`: the row's pictures and files (a reply's links, a message's attachments). */
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
	if (row.kind === 'human') {
		return {
			id: row.id,
			kind: 'human',
			senderName: row.senderName ?? 'Someone',
			text: row.text ?? '',
			attachments: displayAttachments(row, mediaRows),
			queued: row.seq === null,
			createdAt
		};
	}
	if (row.kind === 'tool_results') {
		const blocks = JSON.parse(row.content) as Anthropic.ToolResultBlockParam[];
		return {
			id: row.id,
			kind: 'tool_results',
			results: blocks.map((b) => ({
				id: b.tool_use_id,
				output: toolResultText(b.content),
				isError: b.is_error === true
			})),
			createdAt
		};
	}
	const content = JSON.parse(row.content) as Anthropic.ContentBlock[];
	const blocks: DisplayBlock[] = [];
	for (const block of content) {
		if (block.type === 'text' && block.text.trim()) blocks.push({ type: 'text', text: block.text });
		else if (block.type === 'thinking' && block.thinking.trim()) {
			blocks.push({ type: 'thinking', text: block.thinking });
		} else if (block.type === 'tool_use') {
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
		createdAt
	};
}
