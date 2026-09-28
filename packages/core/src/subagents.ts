import { randomUUID } from 'node:crypto';
import { appendFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { and, asc, eq, inArray } from 'drizzle-orm';
import { EFFORTS, type Effort } from './models.ts';
import {
	committedRows,
	createConversation,
	getConversation,
	insertQueuedNotice,
	isSubagentConversation,
	ModelSwitchError,
	queuedRows,
	replyText,
	setEffort,
	setPreset,
	type Conversation
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { subagent } from './db/schema.ts';
import { profileDir } from './paths.ts';
import { getPreset } from './presets.ts';
import { getProfile } from './profiles.ts';

/**
 * Subagents: agents a conversation's agent starts with `nolune agent run` to work on a task in a
 * hidden conversation of their own, which starts empty but for the task and caches its prompt for
 * 5 minutes. The CLI only writes rows (a subagent, and a queued message in its conversation); the
 * gateway picks them up and runs them (subagent-host.ts), like `nolune wake`.
 */

export type Subagent = typeof subagent.$inferSelect;
export type SubagentStatus = Subagent['status'];

/** Subagents one conversation may have working at once. */
export const MAX_ACTIVE_SUBAGENTS = 5;
const ACTIVE: SubagentStatus[] = ['pending', 'running', 'stopping'];
const NAME = /^[a-z0-9][a-z0-9-]{0,39}$/;

export function isActive(s: Pick<Subagent, 'status'>): boolean {
	return ACTIVE.includes(s.status);
}

export class SubagentError extends Error {}

/**
 * The CLI and the gateway both change subagents, from different processes. Every read-then-write
 * takes the write lock first, so neither can slip in between the other's check and its change.
 */
const LOCKED = { behavior: 'immediate' } as const;

function now(): Date {
	return new Date();
}

export function findSubagent(parentId: string, name: string): Subagent | undefined {
	return getDb()
		.select()
		.from(subagent)
		.where(and(eq(subagent.parentId, parentId), eq(subagent.name, name)))
		.get();
}

function requireSubagent(parentId: string, name: string): Subagent {
	const found = findSubagent(parentId, name);
	if (!found) {
		throw new SubagentError(
			`this conversation has no subagent "${name}". \`nolune agent list\` shows its subagents.`
		);
	}
	return found;
}

export function subagentByConversation(conversationId: string): Subagent | undefined {
	return getDb().select().from(subagent).where(eq(subagent.conversationId, conversationId)).get();
}

export function listSubagents(parentId: string): Subagent[] {
	return getDb()
		.select()
		.from(subagent)
		.where(eq(subagent.parentId, parentId))
		.orderBy(asc(subagent.createdAt))
		.all();
}

/** The ones still working (or about to), for the parent's chat to show. */
export function activeSubagents(parentId: string): Subagent[] {
	return getDb()
		.select()
		.from(subagent)
		.where(and(eq(subagent.parentId, parentId), inArray(subagent.status, ACTIVE)))
		.orderBy(asc(subagent.createdAt))
		.all();
}

export function subagentsWithStatus(status: SubagentStatus): Subagent[] {
	return getDb().select().from(subagent).where(eq(subagent.status, status)).all();
}

export function setSubagentStatus(
	id: string,
	status: SubagentStatus,
	error: string | null = null
): void {
	getDb()
		.update(subagent)
		.set({ status, error, updatedAt: now() })
		.where(eq(subagent.id, id))
		.run();
}

/**
 * Where the gateway writes what a subagent says and runs as it works (no reasoning), so the agent
 * that started it can check on it with `tail`: in the profile folder, by parent conversation.
 */
export function subagentLogPath(s: Pick<Subagent, 'parentId' | 'name'>): string | null {
	const parent = getConversation(s.parentId);
	const profile = parent && getProfile(parent.profileId);
	if (!profile) return null;
	return join(profileDir(profile.slug), 'agents', s.parentId.slice(0, 8), `${s.name}.log`);
}

/** Appends to a subagent's log, creating it. A log that can't be written never stops the work. */
export function appendSubagentLog(s: Pick<Subagent, 'parentId' | 'name'>, text: string): void {
	const path = subagentLogPath(s);
	if (!path) return;
	try {
		mkdirSync(dirname(path), { recursive: true });
		appendFileSync(path, text);
	} catch (err) {
		console.error(`[nolune] could not write the log of subagent ${s.name}:`, err);
	}
}

function taskMessage(name: string, prompt: string): string {
	return `[Task from the nolune agent that started you · you are subagent "${name}"]

${prompt}

You are a subagent. Another nolune agent started you from one of the family's conversations and is waiting for your result. You have none of that conversation, only this message and what you find on the computer. Nobody can answer questions while you work, so work it out yourself; the agent that started you may send you more messages. Don't do anything destructive or irreversible that the task doesn't clearly ask for; say what you would do in your last message instead. Your last message goes back to that agent as your result: give it what it needs to carry on (what you found or did, paths, links, numbers, what didn't work), without retelling your steps.`;
}

function moreWorkMessage(prompt: string): string {
	return `[New task from the nolune agent that started you]

${prompt}

Your last message goes back to it as your result again.`;
}

function steerMessage(text: string): string {
	return `[Message from the nolune agent that started you, while you work]

${text}`;
}

/** `agent-1`, `agent-2`, ...: the first number no subagent of the conversation has used. */
function nextName(parentId: string): string {
	const taken = new Set(listSubagents(parentId).map((s) => s.name));
	for (let n = taken.size + 1; ; n++) {
		if (!taken.has(`agent-${n}`)) return `agent-${n}`;
	}
}

function parentFor(parentId: string): Conversation {
	const parent = getConversation(parentId);
	if (!parent) throw new SubagentError('the conversation that runs this command no longer exists.');
	if (isSubagentConversation(parentId)) {
		throw new SubagentError(
			"a subagent can't start subagents of its own. Do the work yourself, or say in your result what else should be done."
		);
	}
	return parent;
}

function firstLine(text: string, max = 60): string {
	const line = text.trim().split('\n')[0];
	return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}

/**
 * `nolune agent run [name]`: starts a subagent with `prompt` as its task, or gives one that finished
 * more work in the same conversation (it keeps what it learned). Returns at once; the gateway
 * starts it within seconds. It runs on the chat's model and reasoning level unless `presetId` or
 * `effort` say otherwise; more work for one that finished may switch either, like in a chat.
 */
export function runSubagent(input: {
	parentId: string;
	name?: string;
	prompt: string;
	/** A model preset instead of the chat's model (`nolune agent run --preset`). */
	presetId?: string;
	/** A reasoning level instead of the chat's (`nolune agent run --effort`). */
	effort?: Effort;
}): { subagent: Subagent; conversation: Conversation; created: boolean } {
	const prompt = input.prompt.trim();
	if (!prompt) throw new SubagentError('the prompt is empty.');
	if (input.effort !== undefined && !EFFORTS.includes(input.effort)) {
		throw new SubagentError(`the reasoning level must be one of ${EFFORTS.join(', ')}.`);
	}
	const preset = input.presetId ? getPreset(input.presetId) : undefined;
	if (input.presetId && !preset) {
		throw new SubagentError(
			'there is no such model preset. `nolune preset list` shows the ones this computer has.'
		);
	}
	const name = input.name?.trim().toLowerCase();
	if (name !== undefined && !NAME.test(name)) {
		throw new SubagentError(
			`"${input.name}" can't be a subagent id: use up to 40 lowercase letters, digits and hyphens, like flights or agent-2.`
		);
	}
	const parent = parentFor(input.parentId);
	const profile = getProfile(parent.profileId);
	if (!profile) throw new SubagentError('the profile no longer exists.');

	const started = getDb().transaction(() => {
		const existing = name ? findSubagent(parent.id, name) : undefined;
		if (existing) {
			if (isActive(existing)) {
				throw new SubagentError(
					`${existing.name} is still working. Steer it with \`nolune agent steer ${existing.name} --prompt "..."\`, or wait for its result with \`nolune agent watch ${existing.name}\`.`
				);
			}
			const conv = getConversation(existing.conversationId);
			if (!conv) throw new SubagentError(`${existing.name}'s conversation no longer exists.`);
			// Allowed, like in a chat; its next request reads the conversation again without the cache.
			let switched = conv;
			if (preset && preset.id !== conv.presetId) {
				try {
					switched = setPreset(conv.id, preset.id);
				} catch (err) {
					if (err instanceof ModelSwitchError) throw new SubagentError(err.message);
					throw err;
				}
			}
			if (input.effort && input.effort !== conv.effort) setEffort(conv.id, input.effort);
			insertQueuedNotice({
				conversationId: existing.conversationId,
				kind: 'agent_message',
				title: existing.name,
				text: prompt,
				content: moreWorkMessage(prompt)
			});
			setSubagentStatus(existing.id, 'pending');
			return {
				subagent: { ...existing, status: 'pending' as const, error: null },
				conversation: { ...switched, effort: input.effort ?? conv.effort },
				created: false
			};
		}

		const active = activeSubagents(parent.id).length;
		if (active >= MAX_ACTIVE_SUBAGENTS) {
			throw new SubagentError(
				`this conversation already has ${active} subagents working, the most it can have at once. Wait for one to finish (\`nolune agent list\`).`
			);
		}
		const chosen = name ?? nextName(parent.id);
		const conv = createConversation({
			...(preset ? { presetId: preset.id } : { modelOf: parent }),
			profile,
			userId: null,
			effort: input.effort ?? parent.effort,
			title: `${chosen}: ${firstLine(prompt)}`,
			hidden: true,
			folderId: parent.folderId,
			cacheTtl: '5m'
		});
		const row: Subagent = {
			id: randomUUID(),
			parentId: parent.id,
			name: chosen,
			conversationId: conv.id,
			status: 'pending',
			error: null,
			createdAt: now(),
			updatedAt: now()
		};
		getDb().insert(subagent).values(row).run();
		insertQueuedNotice({
			conversationId: conv.id,
			kind: 'agent_message',
			title: chosen,
			text: prompt,
			content: taskMessage(chosen, prompt)
		});
		return { subagent: row, conversation: conv, created: true };
	}, LOCKED);
	// So the agent can look at it right away; the gateway fills it in once it starts.
	appendSubagentLog(started.subagent, '');
	return started;
}

/**
 * `nolune agent steer`: adds a message to a working subagent's conversation. It reads it at its
 * next step, or starts again if it was waiting for its background commands.
 */
export function steerSubagent(input: { parentId: string; name: string; text: string }): Subagent {
	const text = input.text.trim();
	if (!text) throw new SubagentError('the message is empty.');
	return getDb().transaction(() => {
		const found = requireSubagent(input.parentId, input.name);
		if (found.status !== 'pending' && found.status !== 'running') {
			throw new SubagentError(
				found.status === 'stopping'
					? `${found.name} is being stopped.`
					: `${found.name} isn't working (${found.status}). Give it more work with \`nolune agent run ${found.name} --prompt "..."\`.`
			);
		}
		insertQueuedNotice({
			conversationId: found.conversationId,
			kind: 'agent_message',
			title: found.name,
			text,
			content: steerMessage(text)
		});
		// Pending again, so the gateway starts it if it was waiting; a running one reads it anyway.
		setSubagentStatus(found.id, 'pending');
		return { ...found, status: 'pending' as const };
	}, LOCKED);
}

/** `nolune agent stop`: the gateway stops it within seconds. */
export function requestSubagentStop(input: { parentId: string; name: string }): Subagent {
	return getDb().transaction(() => {
		const found = requireSubagent(input.parentId, input.name);
		if (!isActive(found)) {
			throw new SubagentError(`${found.name} isn't working (${found.status}).`);
		}
		const why = 'Stopped by the agent that started it.';
		setSubagentStatus(found.id, 'stopping', why);
		return { ...found, status: 'stopping' as const, error: why };
	}, LOCKED);
}

/**
 * Records how a subagent's loop ended (the gateway, when it ends). Unless it's no longer running
 * (asked to stop, or already pending again), or a message reached it meanwhile: then it's marked
 * pending, for the gateway to start again, and 'again' is returned.
 */
export function settleSubagent(
	conversationId: string,
	status: 'done' | 'failed' | 'stopped',
	error: string | null
): 'settled' | 'again' | 'skipped' {
	return getDb().transaction(() => {
		const found = subagentByConversation(conversationId);
		if (found?.status !== 'running') return 'skipped';
		if (queuedRows(conversationId).length) {
			setSubagentStatus(found.id, 'pending');
			return 'again';
		}
		setSubagentStatus(found.id, status, error);
		return 'settled';
	}, LOCKED);
}

/** Its last message: what it said when it last finished, or while it works, most recently. */
export function lastSubagentMessage(s: Pick<Subagent, 'conversationId'>): string {
	const last = committedRows(s.conversationId).findLast((row) => row.kind === 'assistant');
	return last ? replyText(last) : '';
}

/**
 * What `nolune agent watch` prints once the subagent's current work has ended, or null while it
 * still works. Its last message is its result.
 */
export function subagentResult(s: Subagent): { ok: boolean; text: string } | null {
	if (isActive(s)) return null;
	const last = lastSubagentMessage(s);
	if (s.status === 'done') {
		return { ok: true, text: last || `(${s.name} finished without writing anything.)` };
	}
	const how =
		s.status === 'failed'
			? `${s.name} failed: ${s.error ?? 'unknown error'}`
			: `${s.name} was stopped. ${s.error ?? ''}`;
	return {
		ok: false,
		text: last ? `${how.trim()}\n\nIts last message:\n${last}` : how.trim()
	};
}
