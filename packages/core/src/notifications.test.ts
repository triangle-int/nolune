import { afterEach, describe, expect, it, vi } from 'vitest';
import { committedRows, createConversation, getConversation, replyText } from './conversations.ts';
import {
	continueNotification,
	createNotification,
	dismissAllNotifications,
	dismissNotification,
	listNotificationsForUser,
	markNotificationsSeen,
	onNotificationsChanged,
	pruneNotifications
} from './notifications.ts';
import { addMember, createProfile } from './profiles.ts';
import { makeFamily, makePreset, makeUser } from './test/fixtures.ts';

afterEach(() => {
	vi.useRealTimers();
});

const titles = (userId: string) => listNotificationsForUser(userId).items.map((n) => n.title);

describe('listing notifications', () => {
	it('shows people the notifications of their profiles, newest first', () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 28, 7));
		const { user: anna, profile: family } = makeFamily('Anna');
		const max = makeUser('Max');
		const work = createProfile('Work', max.id);
		addMember(family.id, 'Max');

		createNotification({ profileId: family.id, title: 'Umbrellas', body: 'Rain at 3pm.' });
		vi.advanceTimersByTime(60_000);
		createNotification({
			profileId: work.id,
			title: 'Invoices',
			body: 'Two are due.',
			level: 'error'
		});

		expect(titles(anna.id)).toEqual(['Umbrellas']);
		expect(listNotificationsForUser(max.id).items).toMatchObject([
			{ title: 'Invoices', level: 'error', profile: { slug: 'work', name: 'Work' } },
			{ title: 'Umbrellas', body: 'Rain at 3pm.', level: 'info', profile: { slug: 'family' } }
		]);
	});

	it('remembers when each person last looked', () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 28, 7));
		const { user } = makeFamily();
		expect(listNotificationsForUser(user.id).seenAt).toBe(0);
		markNotificationsSeen(user.id);
		vi.advanceTimersByTime(60_000);
		markNotificationsSeen(user.id);
		expect(listNotificationsForUser(user.id).seenAt).toBe(Date.UTC(2026, 8, 28, 7, 1));
	});

	it('drops old notifications', () => {
		const { user, profile } = makeFamily();
		createNotification({ profileId: profile.id, title: 'Old', body: '' });
		pruneNotifications(new Date(Date.now() + 1000));
		expect(titles(user.id)).toEqual([]);
	});
});

describe('dismissing', () => {
	it('hides a notification only for the person who dismissed it', () => {
		const { user: anna, profile } = makeFamily('Anna');
		const max = makeUser('Max');
		addMember(profile.id, 'Max');
		const n = createNotification({ profileId: profile.id, title: 'Umbrellas', body: '' });
		createNotification({ profileId: profile.id, title: 'Bins', body: '' });

		expect(dismissNotification(n.id, anna.id)).toBe(true);
		expect(dismissNotification(n.id, anna.id)).toBe(true);
		expect(titles(anna.id)).toEqual(['Bins']);
		expect(titles(max.id).sort()).toEqual(['Bins', 'Umbrellas']);

		dismissAllNotifications(max.id);
		expect(titles(max.id)).toEqual([]);
		expect(titles(anna.id)).toEqual(['Bins']);
	});

	it("can't dismiss another profile's notification", () => {
		const { profile } = makeFamily('Anna');
		const n = createNotification({ profileId: profile.id, title: 'Umbrellas', body: '' });
		expect(dismissNotification(n.id, makeUser('Max').id)).toBe(false);
	});
});

describe('continueNotification', () => {
	it("opens the background run's conversation and makes it visible", () => {
		const { user, profile } = makeFamily();
		const run = createConversation({
			profile,
			presetId: makePreset().id,
			userId: null,
			hidden: true
		});
		const n = createNotification({
			profileId: profile.id,
			title: 'Umbrellas',
			body: '',
			conversationId: run.id
		});
		// Nothing to open until then: hidden conversations aren't in the list.
		expect(listNotificationsForUser(user.id).items[0].conversationId).toBeNull();

		expect(continueNotification(n.id, user.id)).toEqual({ slug: 'family', conversationId: run.id });
		expect(getConversation(run.id)?.hidden).toBe(false);
		expect(listNotificationsForUser(user.id).items[0].conversationId).toBe(run.id);
	});

	it('starts a conversation from the notification when there is none', () => {
		const { user, profile } = makeFamily();
		makePreset();
		const n = createNotification({
			profileId: profile.id,
			title: 'Script failed',
			body: 'Exit code 1.',
			level: 'error'
		});

		const opened = continueNotification(n.id, user.id)!;
		const chat = getConversation(opened.conversationId);
		expect(chat).toMatchObject({ title: 'Script failed', hidden: false, createdBy: user.id });
		const rows = committedRows(chat!.id);
		expect(rows.map((r) => r.kind)).toEqual(['trigger', 'assistant']);
		expect(replyText(rows[1])).toBe('Exit code 1.');
		// Opening it again goes to the same conversation.
		expect(continueNotification(n.id, user.id)).toEqual(opened);
	});

	it('refuses people outside the profile, and profiles without models', () => {
		const { user, profile } = makeFamily();
		const n = createNotification({ profileId: profile.id, title: 'Umbrellas', body: '' });
		expect(continueNotification(n.id, makeUser('Max').id)).toBeNull();
		expect(() => continueNotification(n.id, user.id)).toThrow('No models are set up yet.');
	});
});

it('tells listeners which profile changed', () => {
	const { user, profile } = makeFamily();
	makePreset();
	const listener = vi.fn();
	const stop = onNotificationsChanged(listener);
	const n = createNotification({ profileId: profile.id, title: 'Umbrellas', body: '' });
	continueNotification(n.id, user.id);
	stop();
	createNotification({ profileId: profile.id, title: 'Bins', body: '' });
	expect(listener.mock.calls).toEqual([[profile.id], [profile.id]]);
});
