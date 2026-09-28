import { randomUUID } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { and, desc, eq, isNull, lt } from 'drizzle-orm';
import type { Avatar } from './avatars.ts';
import { appendRow, createConversation, getConversation, setHidden } from './conversations.ts';
import { getDb } from './db/index.ts';
import {
	conversation,
	notification,
	notificationDismissal,
	notificationSeen,
	profile,
	profileMember
} from './db/schema.ts';
import { getDefaultPreset, getPreset } from './presets.ts';
import { formatLocalTime, getTrigger } from './triggers.ts';

export type Notification = typeof notification.$inferSelect;

export interface NotificationItem {
	id: string;
	title: string;
	body: string;
	level: Notification['level'];
	createdAt: number;
	profile: { slug: string; name: string; avatar: Avatar };
	/** Set once someone continued it: the conversation to open. */
	conversationId: string | null;
}

const holder = globalThis as unknown as { __noluneNotifications?: EventEmitter };
const emitter = (holder.__noluneNotifications ??= new EventEmitter().setMaxListeners(0));

/** Called with the profile id whenever that profile's notifications change. */
export function onNotificationsChanged(listener: (profileId: string) => void): () => void {
	emitter.on('changed', listener);
	return () => emitter.off('changed', listener);
}

function changed(profileId: string): void {
	emitter.emit('changed', profileId);
}

export function createNotification(input: {
	profileId: string;
	title: string;
	body: string;
	level?: Notification['level'];
	triggerId?: string | null;
	conversationId?: string | null;
}): Notification {
	const row = getDb()
		.insert(notification)
		.values({
			id: randomUUID(),
			profileId: input.profileId,
			triggerId: input.triggerId ?? null,
			conversationId: input.conversationId ?? null,
			title: input.title,
			body: input.body,
			level: input.level ?? 'info',
			createdAt: new Date()
		})
		.returning()
		.get();
	changed(input.profileId);
	return row;
}

function visibleTo(userId: string) {
	return getDb()
		.select({ notification, profile, hidden: conversation.hidden })
		.from(notification)
		.innerJoin(
			profileMember,
			and(eq(profileMember.profileId, notification.profileId), eq(profileMember.userId, userId))
		)
		.innerJoin(profile, eq(profile.id, notification.profileId))
		.leftJoin(conversation, eq(conversation.id, notification.conversationId))
		.leftJoin(
			notificationDismissal,
			and(
				eq(notificationDismissal.notificationId, notification.id),
				eq(notificationDismissal.userId, userId)
			)
		)
		.$dynamic();
}

/** Newest first, from every profile the user is a member of, minus the ones they dismissed. */
export function listNotificationsForUser(
	userId: string,
	limit = 50
): { items: NotificationItem[]; seenAt: number } {
	const rows = visibleTo(userId)
		.where(isNull(notificationDismissal.notificationId))
		.orderBy(desc(notification.createdAt))
		.limit(limit)
		.all();
	const seen = getDb()
		.select()
		.from(notificationSeen)
		.where(eq(notificationSeen.userId, userId))
		.get();
	return {
		items: rows.map(({ notification: n, profile: p, hidden }) => ({
			id: n.id,
			title: n.title,
			body: n.body,
			level: n.level,
			createdAt: n.createdAt.getTime(),
			profile: { slug: p.slug, name: p.name, avatar: p.avatar },
			conversationId: hidden === false ? n.conversationId : null
		})),
		seenAt: seen?.seenAt.getTime() ?? 0
	};
}

export function markNotificationsSeen(userId: string): void {
	const seenAt = new Date();
	getDb()
		.insert(notificationSeen)
		.values({ userId, seenAt })
		.onConflictDoUpdate({ target: notificationSeen.userId, set: { seenAt } })
		.run();
}

/** Hides the notification for this user only; the rest of the profile still sees it. */
export function dismissNotification(id: string, userId: string): boolean {
	const found = visibleTo(userId).where(eq(notification.id, id)).get();
	if (!found) return false;
	getDb()
		.insert(notificationDismissal)
		.values({ notificationId: id, userId })
		.onConflictDoNothing()
		.run();
	return true;
}

export function dismissAllNotifications(userId: string): void {
	const rows = visibleTo(userId).where(isNull(notificationDismissal.notificationId)).all();
	if (rows.length === 0) return;
	getDb()
		.insert(notificationDismissal)
		.values(rows.map((r) => ({ notificationId: r.notification.id, userId })))
		.onConflictDoNothing()
		.run();
}

/**
 * Turns the notification into a normal conversation: the background run it came from, or, when
 * there is none (script failures, or the run was deleted), a new one that starts with its text.
 * Null if the user can't see the notification.
 */
export function continueNotification(
	id: string,
	userId: string
): { slug: string; conversationId: string } | null {
	const found = visibleTo(userId).where(eq(notification.id, id)).get();
	if (!found) return null;
	const { notification: n, profile: p } = found;

	const existing = n.conversationId ? getConversation(n.conversationId) : undefined;
	if (existing) {
		if (existing.hidden) {
			setHidden(existing.id, false);
			changed(n.profileId);
		}
		return { slug: p.slug, conversationId: existing.id };
	}

	const t = n.triggerId ? getTrigger(n.triggerId) : undefined;
	const preset = (t?.presetId && getPreset(t.presetId)) || getDefaultPreset();
	if (!preset) throw new Error('No models are set up yet.');
	const conv = createConversation({
		profile: p,
		presetId: preset.id,
		userId,
		effort: t?.effort,
		title: n.title
	});
	appendRow({
		conversationId: conv.id,
		role: 'user',
		kind: 'trigger',
		senderName: n.title,
		text: 'Opened from a notification.',
		blocks: [
			{
				type: 'text',
				text: `[Notification "${n.title}" · ${formatLocalTime(n.createdAt)}]\n\nEarlier you sent the notification below to everyone in this profile. Someone opened it to continue from there.`
			}
		]
	});
	// nolune's own reply, in its format: any model reads it.
	appendRow({
		conversationId: conv.id,
		role: 'assistant',
		kind: 'assistant',
		blocks: [{ type: 'text', text: n.body }]
	});
	getDb()
		.update(notification)
		.set({ conversationId: conv.id })
		.where(eq(notification.id, n.id))
		.run();
	changed(n.profileId);
	return { slug: p.slug, conversationId: conv.id };
}

export function pruneNotifications(before: Date): void {
	getDb().delete(notification).where(lt(notification.createdAt, before)).run();
}
