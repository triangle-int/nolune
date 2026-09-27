import { randomUUID } from 'node:crypto';
import type Anthropic from '@anthropic-ai/sdk';
import { and, desc, eq, inArray, isNotNull, isNull, lt, max } from 'drizzle-orm';
import { parseAttachments, type MessageAttachment } from './attachments.ts';
import { portableReply, replyBlocks, toolCalls } from './content-blocks.ts';
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
	newMediaId,
	toDisplayMedia,
	type DisplayMedia,
	type MediaRow,
	type PreparedMedia
} from './media.ts';
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
			results: { id: string; output: string; isError: boolean }[];
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

/** Chats on the Claude plan: Claude Code's session, and the last row it was sent. */
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
			content: JSON.stringify([{ type: 'text', text: input.content }]),
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
	/**
	 * Replies: the provider and model that wrote it. Command results: the provider their pictures
	 * were prepared for.
	 */
	provider?: Provider | null;
	model?: string | null;
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

/** Exactly what was stored, so the request prefix is byte-identical to the previous call. */
export function toMessageParam(row: MessageRow): Anthropic.MessageParam {
	return { role: row.role, content: JSON.parse(row.content) };
}

/** The model a request goes to. */
export interface ModelTarget {
	provider: Provider;
	model: string;
}

/**
 * Whether a reply goes to the target model as it was stored. Claude reads the thinking of other
 * Claude models itself (the API leaves out what a model can't read), but not what OpenAI returned
 * or what a Claude plan's Claude Code got (another account). OpenAI's reasoning goes back only to
 * the model that wrote it. Rows without a provider are btw's own text, which every model reads.
 */
function sendsAsStored(row: MessageRow, target: ModelTarget): boolean {
	if (!row.provider) return true;
	if (row.provider !== target.provider) return false;
	return target.provider !== 'openai' || row.model === target.model;
}

type Block = { type?: unknown; source?: { type?: unknown }; content?: unknown };

/** A picture or PDF kept in a provider's Files API, which only that provider can open. */
function isStoredFile(block: Block): boolean {
	return (block.type === 'image' || block.type === 'document') && block.source?.type === 'file';
}

/** What the model reads instead of a picture or PDF another provider holds. */
function storedFileNote(block: Block): Anthropic.TextBlockParam {
	const what = block.type === 'image' ? 'Picture' : 'PDF';
	return {
		type: 'text',
		text: `[${what} not shown: it went to the model this chat used before, and this model can't open that copy. The line before this says where its file is${block.type === 'image' ? '; `btw view` shows it again' : ''}.]`
	};
}

/**
 * A message's or command result's blocks for another provider than the one its pictures and PDFs
 * were uploaded to: each one it can't open becomes a note. Blocks inline as base64 stay, since
 * every provider reads them.
 */
function withoutStoredFiles(blocks: Block[]): Block[] {
	if (!blocks.some((b) => isStoredFile(b) || Array.isArray(b.content))) return blocks;
	return blocks.map((b) => {
		if (isStoredFile(b)) return storedFileNote(b);
		if (b.type !== 'tool_result' || !Array.isArray(b.content)) return b;
		const content = withoutStoredFiles(b.content as Block[]);
		return content === b.content ? b : { ...b, content };
	});
}

/**
 * The transcript as a model call to `target` sends it: every row exactly as stored, except
 *
 * - the thinking in replies from before the system prompt was last built again
 *   (`promptChangedAtSeq`). A thinking block's signature records the prompt it was made under,
 *   and newer models refuse it under another one, so those are left out. They are always the
 *   oldest ones, which the API allows. OpenAI's reasoning isn't bound to the prompt, so its
 *   replies go as they are.
 * - after the conversation switched models, what the target can't read: replies another
 *   provider (or, on OpenAI, another model) wrote go as their text and tool calls, and pictures
 *   and PDFs another provider holds as a note saying where their file is.
 *
 * Both depend only on the stored rows, so every call leaves out the same and the prefix stays
 * byte-identical.
 *
 * Replies are in their provider's own format (see content-blocks.ts), so for OpenAI's
 * conversations the assistant messages hold its output items rather than Anthropic's blocks.
 */
export function requestMessages(
	rows: MessageRow[],
	promptChangedAtSeq: number | null,
	target: ModelTarget
): Anthropic.MessageParam[] {
	const messages = rows.flatMap((row): Anthropic.MessageParam[] => {
		if (row.role !== 'assistant')
			return [{ role: 'user', content: contentFor(row, target.provider) }];
		let content: Anthropic.ContentBlockParam[];
		if (!sendsAsStored(row, target)) content = portableReply(JSON.parse(row.content));
		else if (promptChangedAtSeq === null || row.seq === null || row.seq > promptChangedAtSeq) {
			return [toMessageParam(row)];
		} else {
			content = (JSON.parse(row.content) as Anthropic.ContentBlockParam[]).filter(
				(b) => b.type !== 'thinking' && b.type !== 'redacted_thinking'
			);
		}
		// A reply that was only thinking (cut off, say) has nothing left to send.
		return content.length ? [{ role: 'assistant', content }] : [];
	});
	return pairToolResults(messages);
}

/**
 * A message's or command result's content for `provider`: as stored, except that pictures and
 * PDFs another provider holds become notes (withoutStoredFiles).
 */
export function contentFor(row: MessageRow, provider: Provider): Anthropic.ContentBlockParam[] {
	const content = JSON.parse(row.content) as Anthropic.ContentBlockParam[];
	if (!row.provider || row.provider === provider) return content;
	return withoutStoredFiles(content as Block[]) as Anthropic.ContentBlockParam[];
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
			open = typeof m.content === 'string' ? [] : toolCalls(m.content).map((c) => c.id);
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

function toolResultText(content: Anthropic.ToolResultBlockParam['content']): string {
	if (typeof content === 'string') return content;
	return (content ?? []).map((b) => (b.type === 'text' ? b.text : `[${b.type}]`)).join('\n');
}

/**
 * A row as plain text: what was said, and what commands printed. Pictures, files and the agent's
 * commands themselves are left out.
 */
export function plainText(row: MessageRow): string {
	if (row.role === 'assistant') return replyText(row);
	return (JSON.parse(row.content) as (Anthropic.TextBlockParam | Anthropic.ToolResultBlockParam)[])
		.map((b) =>
			b.type === 'tool_result' ? toolResultText(b.content) : b.type === 'text' ? b.text : ''
		)
		.filter(Boolean)
		.join('\n')
		.trim();
}

/** The text blocks of an assistant row: what the agent said, without thinking or commands. */
export function replyText(row: MessageRow): string {
	return replyBlocks(JSON.parse(row.content))
		.flatMap((b) => (b.type === 'text' ? [b.text] : []))
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
	const blocks: DisplayBlock[] = [];
	for (const block of replyBlocks(JSON.parse(row.content))) {
		if (block.type !== 'tool_call') {
			if (block.text.trim()) blocks.push(block);
		} else {
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
