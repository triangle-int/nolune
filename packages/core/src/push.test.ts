import { randomUUID } from 'node:crypto';
import { eq } from 'drizzle-orm';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { initConfig, updateConfig } from './config.ts';
import { getDb } from './db/index.ts';
import { pushDevice, session } from './db/schema.ts';
import { createNotification } from './notifications.ts';
import { addMember, createProfile } from './profiles.ts';
import {
	forgetSessionPushDevice,
	isDeviceToken,
	notificationText,
	pushDevicesForProfile,
	pushNotification,
	registerPushDevice
} from './push.ts';
import { makeFamily, makeUser } from './test/fixtures.ts';

const PHONE = 'a1'.repeat(32);
const OTHER_PHONE = 'b2'.repeat(32);

afterEach(() => {
	vi.unstubAllGlobals();
	vi.restoreAllMocks();
});

/** A session for the user, as signing in makes. */
function signIn(userId: string): string {
	const id = randomUUID();
	getDb()
		.insert(session)
		.values({
			id,
			userId,
			token: randomUUID(),
			expiresAt: new Date(Date.now() + 7 * 86_400_000),
			updatedAt: new Date()
		})
		.run();
	return id;
}

function useRelay(): void {
	initConfig();
	updateConfig((c) => {
		c.relay = {
			server: 'https://relay.test',
			name: 'smiths',
			url: 'https://smiths.nolune.test',
			token: 'secret'
		};
	});
}

/** Stands in for the relay, answering `answer`, and keeps what it was sent. */
function fakeRelay(answer: () => Response = () => Response.json({ sent: 1, gone: [] })) {
	const requests: { url: string; headers: Headers; body: Record<string, unknown> }[] = [];
	vi.stubGlobal(
		'fetch',
		vi.fn(async (url: URL, init: RequestInit) => {
			requests.push({
				url: String(url),
				headers: new Headers(init.headers),
				body: JSON.parse(String(init.body))
			});
			return answer();
		})
	);
	return requests;
}

describe('registering an iPhone', () => {
	it('keeps one for each session, and moves it to whoever signs in on it', () => {
		const { user: anna, profile } = makeFamily('Anna');
		const max = makeUser('Max');
		addMember(profile.id, 'Max');
		const annas = signIn(anna.id);
		registerPushDevice({ token: PHONE, sandbox: false, userId: anna.id, sessionId: annas });
		expect(pushDevicesForProfile(profile.id)).toEqual([{ token: PHONE, sandbox: false }]);

		// The app got a new token from Apple.
		registerPushDevice({
			token: OTHER_PHONE.toUpperCase(),
			sandbox: true,
			userId: anna.id,
			sessionId: annas
		});
		expect(pushDevicesForProfile(profile.id)).toEqual([{ token: OTHER_PHONE, sandbox: true }]);

		// Anna signed out on it, and Max in.
		registerPushDevice({
			token: OTHER_PHONE,
			sandbox: true,
			userId: max.id,
			sessionId: signIn(max.id)
		});
		const rows = getDb().select().from(pushDevice).all();
		expect(rows).toMatchObject([{ token: OTHER_PHONE, userId: max.id }]);
	});

	it('forgets it when the app leaves for another nolune', () => {
		const { user, profile } = makeFamily();
		const sessionId = signIn(user.id);
		registerPushDevice({ token: PHONE, sandbox: false, userId: user.id, sessionId });
		registerPushDevice({
			token: OTHER_PHONE,
			sandbox: false,
			userId: user.id,
			sessionId: signIn(user.id)
		});
		forgetSessionPushDevice(sessionId);
		expect(pushDevicesForProfile(profile.id)).toEqual([{ token: OTHER_PHONE, sandbox: false }]);
	});

	it('forgets it when that session signs out', () => {
		const { user, profile } = makeFamily();
		const sessionId = signIn(user.id);
		registerPushDevice({ token: PHONE, sandbox: false, userId: user.id, sessionId });
		getDb().delete(session).where(eq(session.id, sessionId)).run();
		expect(pushDevicesForProfile(profile.id)).toEqual([]);
	});

	it('takes only tokens from Apple', () => {
		expect(isDeviceToken(PHONE)).toBe(true);
		expect(isDeviceToken(PHONE.toUpperCase())).toBe(true);
		for (const bad of ['', 'ab', 'xyz'.repeat(30), 42, null])
			expect(isDeviceToken(bad)).toBe(false);
	});
});

describe("a notification on members' iPhones", () => {
	it('goes through the relay to the iPhones of the profile, with the bell to open', async () => {
		useRelay();
		const requests = fakeRelay();
		const { user: anna, profile } = makeFamily('Anna');
		const max = makeUser('Max');
		const leo = makeUser('Leo');
		addMember(profile.id, 'Max');
		createProfile('Work', leo.id);
		registerPushDevice({
			token: PHONE,
			sandbox: false,
			userId: anna.id,
			sessionId: signIn(anna.id)
		});
		registerPushDevice({
			token: OTHER_PHONE,
			sandbox: true,
			userId: max.id,
			sessionId: signIn(max.id)
		});
		registerPushDevice({
			token: 'c3'.repeat(32),
			sandbox: false,
			userId: leo.id,
			sessionId: signIn(leo.id)
		});

		const n = createNotification({
			profileId: profile.id,
			title: 'Umbrellas',
			body: '**Rain** at 3pm.'
		});
		await vi.waitFor(() => expect(requests).toHaveLength(1));

		const [{ url, headers, body }] = requests;
		expect(url).toBe('https://relay.test/api/gateways/smiths/push');
		expect(headers.get('authorization')).toBe('Bearer secret');
		expect(body).toEqual({
			devices: expect.arrayContaining([
				{ token: PHONE, sandbox: false },
				{ token: OTHER_PHONE, sandbox: true }
			]),
			title: 'Umbrellas',
			subtitle: 'Family',
			body: 'Rain at 3pm.',
			thread: 'family',
			path: `/p/family?notification=${n.id}`
		});
		expect((body.devices as unknown[]).length).toBe(2);
	});

	it('forgets the iPhones Apple says are gone', async () => {
		useRelay();
		fakeRelay(() => Response.json({ sent: 1, gone: [OTHER_PHONE] }));
		const { user, profile } = makeFamily();
		registerPushDevice({
			token: PHONE,
			sandbox: false,
			userId: user.id,
			sessionId: signIn(user.id)
		});
		registerPushDevice({
			token: OTHER_PHONE,
			sandbox: false,
			userId: user.id,
			sessionId: signIn(user.id)
		});
		await pushNotification(createNotificationQuietly(profile.id));
		expect(pushDevicesForProfile(profile.id)).toEqual([{ token: PHONE, sandbox: false }]);
	});

	it("isn't sent without the relay, or anyone's iPhone, and a refusal is only logged", async () => {
		const requests = fakeRelay(() => Response.json({ error: 'no key' }, { status: 501 }));
		const error = vi.spyOn(console, 'error').mockImplementation(() => {});
		const { user, profile } = makeFamily();
		initConfig();
		registerPushDevice({
			token: PHONE,
			sandbox: false,
			userId: user.id,
			sessionId: signIn(user.id)
		});
		await pushNotification(createNotificationQuietly(profile.id));
		expect(requests).toHaveLength(0);

		useRelay();
		const other = createProfile('Work', makeUser('Leo').id);
		await pushNotification(createNotificationQuietly(other.id));
		expect(requests).toHaveLength(0);

		await pushNotification(createNotificationQuietly(profile.id));
		expect(requests).toHaveLength(1);
		expect(error).toHaveBeenCalledWith(
			"[nolune] the relay didn't send a notification to iPhones: no key"
		);
		expect(pushDevicesForProfile(profile.id)).toHaveLength(1);
	});
});

/** A notification row, without createNotification sending it on by itself. */
function createNotificationQuietly(profileId: string) {
	return {
		id: randomUUID(),
		profileId,
		triggerId: null,
		conversationId: null,
		title: 'Umbrellas',
		body: 'Rain at 3pm.',
		level: 'info' as const,
		createdAt: new Date()
	};
}

describe("a notification's text on the lock screen", () => {
	it('has no Markdown: pictures as their description, links as their label', () => {
		expect(
			notificationText(
				[
					'# Rain today',
					'',
					'Take **umbrellas** and _boots_, see [the forecast](https://example.com).',
					'',
					'![The radar](/tmp/radar.png) and [the file](report.pdf)',
					'',
					'- Anna: `school`',
					'- Max: work',
					'',
					'1. one',
					'2. two',
					'',
					'> quoted',
					'',
					'| Day | Rain |',
					'| --- | ---- |',
					'| Mon | 80% |',
					'',
					'```',
					'code here',
					'```',
					'',
					'---',
					'Line<br>break \\*not bold\\* <b>html</b>'
				].join('\n')
			)
		).toBe(
			[
				'Rain today',
				'Take umbrellas and boots, see the forecast.',
				'[The radar] and the file',
				'• Anna: school',
				'• Max: work',
				'1. one',
				'2. two',
				'quoted',
				'Day · Rain',
				'Mon · 80%',
				'code here',
				'Linebreak *not bold* html'
			].join('\n')
		);
	});

	it('is cut where the lock screen would never get to', () => {
		const text = notificationText('word '.repeat(1000));
		expect([...text].length).toBeLessThanOrEqual(2001);
		expect([...text].length).toBeGreaterThan(1990);
		expect(text.endsWith('…')).toBe(true);
	});
});
