import type Anthropic from '@anthropic-ai/sdk';
import { relations, sql } from 'drizzle-orm';
import {
	sqliteTable,
	text,
	integer,
	index,
	primaryKey,
	uniqueIndex
} from 'drizzle-orm/sqlite-core';
import { AVATARS } from '../avatars.ts';

const now = sql`(cast(unixepoch('subsecond') * 1000 as integer))`;

// --- better-auth tables (keep in sync with the better-auth config in src/lib/server/auth.ts) ---

export const user = sqliteTable('user', {
	id: text('id').primaryKey(),
	name: text('name').notNull(),
	email: text('email').notNull().unique(),
	emailVerified: integer('email_verified', { mode: 'boolean' }).default(false).notNull(),
	image: text('image'),
	isAdmin: integer('is_admin', { mode: 'boolean' }).default(false).notNull(),
	createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
	updatedAt: integer('updated_at', { mode: 'timestamp_ms' })
		.default(now)
		.$onUpdate(() => /* @__PURE__ */ new Date())
		.notNull()
});

export const session = sqliteTable(
	'session',
	{
		id: text('id').primaryKey(),
		expiresAt: integer('expires_at', { mode: 'timestamp_ms' }).notNull(),
		token: text('token').notNull().unique(),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
		updatedAt: integer('updated_at', { mode: 'timestamp_ms' })
			.$onUpdate(() => /* @__PURE__ */ new Date())
			.notNull(),
		ipAddress: text('ip_address'),
		userAgent: text('user_agent'),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' })
	},
	(table) => [index('session_userId_idx').on(table.userId)]
);

export const account = sqliteTable(
	'account',
	{
		id: text('id').primaryKey(),
		accountId: text('account_id').notNull(),
		providerId: text('provider_id').notNull(),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		accessToken: text('access_token'),
		refreshToken: text('refresh_token'),
		idToken: text('id_token'),
		accessTokenExpiresAt: integer('access_token_expires_at', { mode: 'timestamp_ms' }),
		refreshTokenExpiresAt: integer('refresh_token_expires_at', { mode: 'timestamp_ms' }),
		scope: text('scope'),
		password: text('password'),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
		updatedAt: integer('updated_at', { mode: 'timestamp_ms' })
			.$onUpdate(() => /* @__PURE__ */ new Date())
			.notNull()
	},
	(table) => [index('account_userId_idx').on(table.userId)]
);

export const verification = sqliteTable(
	'verification',
	{
		id: text('id').primaryKey(),
		identifier: text('identifier').notNull(),
		value: text('value').notNull(),
		expiresAt: integer('expires_at', { mode: 'timestamp_ms' }).notNull(),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
		updatedAt: integer('updated_at', { mode: 'timestamp_ms' })
			.default(now)
			.$onUpdate(() => /* @__PURE__ */ new Date())
			.notNull()
	},
	(table) => [index('verification_identifier_idx').on(table.identifier)]
);

export const userRelations = relations(user, ({ many }) => ({
	sessions: many(session),
	accounts: many(account)
}));

export const sessionRelations = relations(session, ({ one }) => ({
	user: one(user, { fields: [session.userId], references: [user.id] })
}));

export const accountRelations = relations(account, ({ one }) => ({
	user: one(user, { fields: [account.userId], references: [user.id] })
}));

// --- app tables ---

export const profile = sqliteTable('profile', {
	id: text('id').primaryKey(),
	/** Folder name under ~/.btw-agent/profiles. Fixed at creation. */
	slug: text('slug').notNull().unique(),
	name: text('name').notNull(),
	/**
	 * The assistant's mascot in this profile. Profiles start with one picked from the slug
	 * (defaultAvatar); the SQL default only lets the column be added to existing rows.
	 */
	avatar: text('avatar', { enum: AVATARS }).notNull().default('probe'),
	/** Skill names left out of new chats' prompts. Skills are on unless listed, new ones included. */
	disabledSkills: text('disabled_skills', { mode: 'json' })
		.$type<string[]>()
		.notNull()
		.default(sql`'[]'`),
	/**
	 * Whether btw looks over its chats once they go quiet and saves what's worth remembering
	 * (memory-learning.ts), besides what the agent saves itself.
	 */
	learnFromChats: integer('learn_from_chats', { mode: 'boolean' }).notNull().default(true),
	createdBy: text('created_by').references(() => user.id, { onDelete: 'set null' }),
	createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
});

export const profileMember = sqliteTable(
	'profile_member',
	{
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		addedAt: integer('added_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [
		primaryKey({ columns: [table.profileId, table.userId] }),
		index('profile_member_userId_idx').on(table.userId)
	]
);

export const modelPreset = sqliteTable('model_preset', {
	id: text('id').primaryKey(),
	name: text('name').notNull().unique(),
	provider: text('provider', {
		enum: ['anthropic', 'openai', 'openrouter', 'claude-plan', 'chatgpt-plan']
	}).notNull(),
	model: text('model').notNull(),
	/** Admin override. Wins over modelContextWindow. */
	contextWindow: integer('context_window'),
	/** From the provider's models API when the preset was created. */
	modelContextWindow: integer('model_context_window'),
	/** Picked for new chats and automations. At most one; with none, the oldest preset is used. */
	isDefault: integer('is_default', { mode: 'boolean' }).notNull().default(false),
	createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
});

/**
 * A folder of chats in a profile, like a project: its chats share its instructions and files,
 * which are part of their system prompt.
 */
export const folder = sqliteTable(
	'folder',
	{
		id: text('id').primaryKey(),
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		/** Its files are saved in `profiles/<profile>/folders/<slug>`. Fixed at creation. */
		slug: text('slug').notNull(),
		name: text('name').notNull(),
		/** What btw should know or do in every chat of the folder, in the family's words. */
		instructions: text('instructions').notNull().default(''),
		createdBy: text('created_by').references(() => user.id, { onDelete: 'set null' }),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [uniqueIndex('folder_profile_slug_idx').on(table.profileId, table.slug)]
);

/**
 * A file the family added to a folder. The agent gets its path (a copy in the folder's own
 * folder); the page shows the original from the media store.
 */
export const folderFile = sqliteTable(
	'folder_file',
	{
		id: text('id').primaryKey(),
		folderId: text('folder_id')
			.notNull()
			.references(() => folder.id, { onDelete: 'cascade' }),
		name: text('name').notNull(),
		/** The copy the agent works with. */
		path: text('path').notNull(),
		/** The original: `~/.btw-agent/media/<sha256>`. */
		sha256: text('sha256').notNull(),
		/** Sniffed from the content. */
		mime: text('mime').notNull(),
		bytes: integer('bytes').notNull(),
		/** Pictures: pixel size as displayed, when it could be read. */
		width: integer('width'),
		height: integer('height'),
		/** A JPEG copy for pictures browsers can't show (HEIC, TIFF). */
		previewSha256: text('preview_sha256'),
		createdBy: text('created_by').references(() => user.id, { onDelete: 'set null' }),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [index('folder_file_folderId_idx').on(table.folderId)]
);

export const conversation = sqliteTable(
	'conversation',
	{
		id: text('id').primaryKey(),
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		title: text('title').notNull().default(''),
		presetId: text('preset_id').references(() => modelPreset.id, { onDelete: 'set null' }),
		/**
		 * Snapshot of the preset the conversation runs on, taken when it was created or last
		 * switched to another model (setPreset), so later changes to the preset leave it alone.
		 */
		presetName: text('preset_name').notNull(),
		provider: text('provider', {
			enum: ['anthropic', 'openai', 'openrouter', 'claude-plan', 'chatgpt-plan']
		}).notNull(),
		model: text('model').notNull(),
		contextWindow: integer('context_window'),
		effort: text('effort', { enum: ['low', 'medium', 'high', 'xhigh', 'max'] })
			.notNull()
			.default('medium'),
		/**
		 * Frozen at creation so the prompt cache prefix never changes, except when the chat moves
		 * to another folder, its folder's instructions or files change, or the profile's soul
		 * changes: then it is built again at the start of the next turn.
		 */
		systemPrompt: text('system_prompt').notNull(),
		folderId: text('folder_id').references(() => folder.id, { onDelete: 'set null' }),
		/** The folder's part of `systemPrompt` ('' outside a folder), to tell when it's out of date. */
		folderContext: text('folder_context').notNull().default(''),
		/** The profile's soul as `systemPrompt` has it ('' without one), to tell when it's out of date. */
		soul: text('soul').notNull().default(''),
		/**
		 * The last row before the system prompt was built again. Thinking in rows up to it belongs
		 * to the old prompt, and the API refuses it under a new one, so requests leave it out.
		 */
		promptChangedAtSeq: integer('prompt_changed_at_seq'),
		/**
		 * The tool definitions its requests send, frozen at creation like `systemPrompt`: a thinking
		 * block is bound to the tools it was made with, so a new version of btw that changes them
		 * only reaches new chats. Null: chats from before this was saved (LEGACY_TOOLS).
		 */
		tools: text('tools', { mode: 'json' }).$type<Anthropic.Tool[]>(),
		/**
		 * Chats on a plan (plans.ts): the session of the plan's agent (Claude Code's, or Codex's
		 * thread) that holds the model's side of the chat, and the last row it has been sent. Null
		 * until its first turn starts.
		 */
		providerSession: text('provider_session', { mode: 'json' }).$type<{
			id: string;
			sentSeq: number;
			/**
			 * The plan whose agent has the session, since a chat can switch between them. Missing:
			 * the Claude plan's, from before there was another.
			 */
			provider?: 'claude-plan' | 'chatgpt-plan';
		}>(),
		/** Prompt cache lifetime: an hour for chats people come back to, 5 minutes for subagents. */
		cacheTtl: text('cache_ttl', { enum: ['5m', '1h'] })
			.notNull()
			.default('1h'),
		/**
		 * Background runs started by triggers, and subagents, stay out of the list. A background run
		 * joins it once someone continues it.
		 */
		hidden: integer('hidden', { mode: 'boolean' }).notNull().default(false),
		/** The last row btw has looked over for memory (memory-learning.ts). Null: none yet. */
		learnedSeq: integer('learned_seq'),
		createdBy: text('created_by').references(() => user.id, { onDelete: 'set null' }),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
		updatedAt: integer('updated_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [
		index('conversation_profileId_idx').on(table.profileId),
		index('conversation_folderId_idx').on(table.folderId)
	]
);

export const message = sqliteTable(
	'message',
	{
		id: integer('id').primaryKey({ autoIncrement: true }),
		conversationId: text('conversation_id')
			.notNull()
			.references(() => conversation.id, { onDelete: 'cascade' }),
		/** Position in the transcript. Null while the message is still queued. */
		seq: integer('seq'),
		role: text('role', { enum: ['user', 'assistant'] }).notNull(),
		/**
		 * Written by the gateway, not a person: `trigger`, the first message of a background run;
		 * `agent_message`, a subagent's task or a steer from the agent that started it;
		 * `task_result`, what a background command printed, once it ended.
		 */
		kind: text('kind', {
			enum: ['human', 'trigger', 'agent_message', 'task_result', 'tool_results', 'assistant']
		}).notNull(),
		senderId: text('sender_id').references(() => user.id, { onDelete: 'set null' }),
		/**
		 * Sender's display name when the message was sent. Trigger rows: the trigger's name. Agent
		 * messages: the subagent's id. Task results: the command's summary.
		 */
		senderName: text('sender_name'),
		/**
		 * What the human typed (without the "Name: " prefix). Trigger rows: the trigger's prompt.
		 * Agent messages: what the agent wrote. Task results: the command's output.
		 */
		text: text('text'),
		/**
		 * JSON. btw's own rows in btw's format (`format` 'btw'), replies exactly as their provider
		 * returned them, and rows from before btw's format in Anthropic's (format.ts). Never
		 * rewritten: what a model got before goes to it again byte for byte.
		 */
		content: text('content').notNull(),
		/** 'btw': `content` is in btw's own format. Null: a reply as it came, or a row from before. */
		format: text('format', { enum: ['btw'] }),
		/**
		 * The provider `content` was made for: the one whose model wrote a reply, or whose Files API a
		 * message's or command result's pictures and PDFs went to. A conversation can switch models,
		 * so another provider's encoder leaves out what it can't take (format.ts). Rows that are only
		 * text, which every provider reads the same, may have none.
		 */
		provider: text('provider', {
			enum: ['anthropic', 'openai', 'openrouter', 'claude-plan', 'chatgpt-plan']
		}),
		/** Replies: the model that wrote it. */
		model: text('model'),
		stopReason: text('stop_reason'),
		usage: text('usage'),
		/**
		 * Human rows: the files attached to the message, in order, as `MessageAttachment[]` JSON.
		 * Provider-neutral, unlike `content`, which says the same in the provider's own format.
		 */
		attachments: text('attachments'),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [index('message_conversation_seq_idx').on(table.conversationId, table.seq)]
);

/**
 * A picture (`![alt](src)`) or file (`[label](src)`) in one of the agent's replies, copied when
 * the reply was saved so the chat keeps showing it after the original moves or disappears.
 */
export const media = sqliteTable(
	'media',
	{
		/** Random; the only handle the web UI gets. */
		id: text('id').primaryKey(),
		conversationId: text('conversation_id')
			.notNull()
			.references(() => conversation.id, { onDelete: 'cascade' }),
		messageId: integer('message_id')
			.notNull()
			.references(() => message.id, { onDelete: 'cascade' }),
		/** The link target exactly as the Markdown lexer read it from the reply. */
		src: text('src').notNull(),
		status: text('status', {
			enum: ['ok', 'missing', 'unsupported', 'too_large', 'blocked', 'failed']
		}).notNull(),
		/** What went wrong, in plain words, when status isn't `ok`. */
		error: text('error'),
		/** File name for downloads. */
		name: text('name').notNull(),
		/** The original, byte for byte: `~/.btw-agent/media/<sha256>`. */
		sha256: text('sha256'),
		mime: text('mime'),
		bytes: integer('bytes'),
		/** Pictures: pixel size as displayed (EXIF rotation applied), when it could be read. */
		width: integer('width'),
		height: integer('height'),
		/** A full-size JPEG copy for pictures browsers can't show (HEIC, TIFF). */
		previewSha256: text('preview_sha256'),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [
		index('media_messageId_idx').on(table.messageId),
		index('media_conversationId_idx').on(table.conversationId)
	]
);

export const trigger = sqliteTable(
	'trigger',
	{
		id: text('id').primaryKey(),
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		name: text('name').notNull(),
		/** What it does in one plain sentence, for the family on the Automations page. */
		summary: text('summary'),
		/** A Lucide icon name (`umbrella`) for the Automations page. */
		icon: text('icon'),
		/** When it fires: on a cron schedule, once at `runAt`, or when its webhook URL is called. */
		kind: text('kind', { enum: ['cron', 'once', 'webhook'] }).notNull(),
		/** 5-field cron expression in the gateway's local time zone. */
		cron: text('cron'),
		runAt: integer('run_at', { mode: 'timestamp_ms' }),
		webhookToken: text('webhook_token').unique(),
		/** What it does: wake the agent with `prompt`, or run `command` without the model. */
		action: text('action', { enum: ['agent', 'script'] }).notNull(),
		prompt: text('prompt'),
		command: text('command'),
		presetId: text('preset_id').references(() => modelPreset.id, { onDelete: 'set null' }),
		effort: text('effort', { enum: ['low', 'medium', 'high', 'xhigh', 'max'] })
			.notNull()
			.default('medium'),
		enabled: integer('enabled', { mode: 'boolean' }).notNull().default(true),
		/** The scheduler fires the trigger once this has passed. Null: nothing scheduled. */
		nextRunAt: integer('next_run_at', { mode: 'timestamp_ms' }),
		lastRunAt: integer('last_run_at', { mode: 'timestamp_ms' }),
		createdBy: text('created_by').references(() => user.id, { onDelete: 'set null' }),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [
		index('trigger_profileId_idx').on(table.profileId),
		index('trigger_nextRunAt_idx').on(table.nextRunAt)
	]
);

export const triggerRun = sqliteTable(
	'trigger_run',
	{
		id: text('id').primaryKey(),
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		/** Null for `btw wake` outside a trigger, or after the trigger was deleted. */
		triggerId: text('trigger_id').references(() => trigger.id, { onDelete: 'set null' }),
		/** The trigger's name at the time; titles the notification and the conversation. */
		title: text('title').notNull(),
		action: text('action', { enum: ['agent', 'script'] }).notNull(),
		source: text('source', { enum: ['cron', 'once', 'webhook', 'wake', 'manual'] }).notNull(),
		status: text('status', {
			enum: ['pending', 'running', 'ok', 'notified', 'silent', 'stopped', 'failed']
		}).notNull(),
		/** Agent runs: what the agent is asked to do (the trigger's prompt or the `btw wake` text). */
		prompt: text('prompt'),
		/** The webhook request body, for runs started by a webhook. */
		payload: text('payload'),
		presetId: text('preset_id').references(() => modelPreset.id, { onDelete: 'set null' }),
		effort: text('effort', { enum: ['low', 'medium', 'high', 'xhigh', 'max'] })
			.notNull()
			.default('medium'),
		/** Agent runs: the hidden conversation the run happens in. */
		conversationId: text('conversation_id').references(() => conversation.id, {
			onDelete: 'set null'
		}),
		/** Script runs: the end of the command's output. Failed runs: what went wrong. */
		output: text('output'),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
		startedAt: integer('started_at', { mode: 'timestamp_ms' }),
		finishedAt: integer('finished_at', { mode: 'timestamp_ms' })
	},
	(table) => [
		index('trigger_run_triggerId_idx').on(table.triggerId, table.createdAt),
		index('trigger_run_status_idx').on(table.status),
		index('trigger_run_conversationId_idx').on(table.conversationId)
	]
);

/**
 * A command the agent started with `run_in_background` that hasn't ended yet. The process lives in
 * the gateway; the row is there so that after a restart the conversation is told it was cut off.
 */
export const backgroundCommand = sqliteTable(
	'background_command',
	{
		/** The run_command call that started it. */
		toolUseId: text('tool_use_id').primaryKey(),
		conversationId: text('conversation_id')
			.notNull()
			.references(() => conversation.id, { onDelete: 'cascade' }),
		summary: text('summary'),
		command: text('command').notNull(),
		startedAt: integer('started_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [index('background_command_conversationId_idx').on(table.conversationId)]
);

/**
 * Another agent that a conversation's agent started with `btw agent run`: it works in a hidden
 * conversation of its own, which starts empty but for its task, and the agent that started it
 * waits for its last message with `btw agent watch`.
 */
export const subagent = sqliteTable(
	'subagent',
	{
		id: text('id').primaryKey(),
		/** The conversation whose agent started it. */
		parentId: text('parent_id')
			.notNull()
			.references(() => conversation.id, { onDelete: 'cascade' }),
		/** What that agent calls it (`agent-1`, or a name it chose); unique in the parent. */
		name: text('name').notNull(),
		/** Its own hidden conversation. */
		conversationId: text('conversation_id')
			.notNull()
			.references(() => conversation.id, { onDelete: 'cascade' }),
		/**
		 * `pending`: it has messages the gateway hasn't started working on (a new task or a steer);
		 * `running`: working, or waiting for its background commands; `done`: its last message is
		 * its result; `stopping`: `btw agent stop` asked the gateway to stop it.
		 */
		status: text('status', {
			enum: ['pending', 'running', 'done', 'failed', 'stopping', 'stopped']
		}).notNull(),
		/** Failed: what went wrong. Stopped: who stopped it. */
		error: text('error'),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull(),
		updatedAt: integer('updated_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [
		uniqueIndex('subagent_parent_name_idx').on(table.parentId, table.name),
		index('subagent_conversationId_idx').on(table.conversationId),
		index('subagent_status_idx').on(table.status)
	]
);

export const notification = sqliteTable(
	'notification',
	{
		id: text('id').primaryKey(),
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		triggerId: text('trigger_id').references(() => trigger.id, { onDelete: 'set null' }),
		/** The run's hidden conversation; "Continue in chat" makes it visible. */
		conversationId: text('conversation_id').references(() => conversation.id, {
			onDelete: 'set null'
		}),
		title: text('title').notNull(),
		body: text('body').notNull(),
		level: text('level', { enum: ['info', 'error'] })
			.notNull()
			.default('info'),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [index('notification_profileId_idx').on(table.profileId, table.createdAt)]
);

/** Notifications are shared by the profile; each member can dismiss them for themselves. */
export const notificationDismissal = sqliteTable(
	'notification_dismissal',
	{
		notificationId: text('notification_id')
			.notNull()
			.references(() => notification.id, { onDelete: 'cascade' }),
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' })
	},
	(table) => [primaryKey({ columns: [table.notificationId, table.userId] })]
);

/** When each user last opened the notification menu. Newer notifications count as unread. */
export const notificationSeen = sqliteTable('notification_seen', {
	userId: text('user_id')
		.primaryKey()
		.references(() => user.id, { onDelete: 'cascade' }),
	seenAt: integer('seen_at', { mode: 'timestamp_ms' }).notNull()
});

/**
 * Files attached in the composer that aren't sent yet. The bytes are already in the media store;
 * sending the message turns them into attachments. Rows older than a day are dropped.
 */
export const upload = sqliteTable(
	'upload',
	{
		id: text('id').primaryKey(),
		profileId: text('profile_id')
			.notNull()
			.references(() => profile.id, { onDelete: 'cascade' }),
		/** Only the person who uploaded a file can send it. */
		userId: text('user_id')
			.notNull()
			.references(() => user.id, { onDelete: 'cascade' }),
		/** The file's name on the person's device, cleaned up. */
		name: text('name').notNull(),
		sha256: text('sha256').notNull(),
		/** Sniffed from the content. */
		mime: text('mime').notNull(),
		bytes: integer('bytes').notNull(),
		createdAt: integer('created_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [index('upload_profileId_idx').on(table.profileId)]
);

/**
 * Files uploaded to a provider (Anthropic's, OpenAI's or OpenRouter's Files API), so requests
 * refer to them by id instead of carrying their bytes. One upload per content and account; the
 * hourly prune deletes the ones no message refers to any more.
 */
export const providerFile = sqliteTable(
	'provider_file',
	{
		provider: text('provider', { enum: ['anthropic', 'openai', 'openrouter'] }).notNull(),
		/** The account the file lives in (a hash of the API key): ids are only valid there. */
		account: text('account').notNull(),
		/** SHA-256 of the bytes that were uploaded. */
		sha256: text('sha256').notNull(),
		fileId: text('file_id').notNull(),
		/** When a message last took this id. The prune leaves files used in the last hour alone. */
		usedAt: integer('used_at', { mode: 'timestamp_ms' }).default(now).notNull()
	},
	(table) => [
		primaryKey({ columns: [table.provider, table.account, table.sha256] }),
		index('provider_file_fileId_idx').on(table.fileId)
	]
);
