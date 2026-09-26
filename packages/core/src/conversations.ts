import { randomUUID } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import { and, desc, eq, isNotNull, isNull, lt, max } from 'drizzle-orm';
import type { Effort } from './anthropic.ts';
import { getDb } from './db/index.ts';
import { conversation, media, message, profile, profileMember, triggerRun } from './db/schema.ts';
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
	  }
	| ({ type: 'memory'; id: string } & MemoryCall);

/** A memory tool call, as far as the chat shows it. */
export interface MemoryCall {
	/** view, create, str_replace, insert, delete or rename. */
	command: string | null;
	/** The file or folder under /memories; the old path for rename. */
	path: string | null;
	/** rename: where it moves to. */
	newPath?: string;
	/** What gets written: the whole file (create), the new text (str_replace) or the lines (insert). */
	text?: string;
	/** str_replace: the text it replaces. */
	oldText?: string;
}

export type DisplayMessage =
	| {
			id: number;
			kind: 'human';
			senderName: string;
			text: string;
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
}): Conversation {
	const preset = getPreset(input.presetId);
	if (!preset) throw new Error('Unknown model preset');
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
		systemPrompt: buildSystemPrompt(input.profile),
		memoryTool: true,
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
}): MessageRow {
	return getDb()
		.insert(message)
		.values({
			conversationId: input.conversationId,
			seq: null,
			role: 'user',
			kind: 'human',
			senderId: input.senderId,
			senderName: input.senderName,
			text: input.text,
			// The model sees only the sender's name and what they wrote.
			content: JSON.stringify([{ type: 'text', text: `${input.senderName}: ${input.text}` }]),
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

function toMemoryCall(input: unknown): MemoryCall {
	const args = (input ?? {}) as Record<string, unknown>;
	const str = (key: string) => (typeof args[key] === 'string' ? (args[key] as string) : undefined);
	const newText = str('file_text') ?? str('new_str') ?? str('insert_text');
	return {
		command: str('command') ?? null,
		path: str('path') ?? str('old_path') ?? null,
		...(str('new_path') !== undefined ? { newPath: str('new_path') } : {}),
		...(newText !== undefined ? { text: newText } : {}),
		...(str('old_str') !== undefined ? { oldText: str('old_str') } : {})
	};
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

/** `mediaRows`: the row's pictures and files, for assistant rows. */
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
		} else if (block.type === 'tool_use' && block.name === 'memory') {
			blocks.push({ type: 'memory', id: block.id, ...toMemoryCall(block.input) });
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
