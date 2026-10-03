import { isHttpError } from '@sveltejs/kit';
import { NOLUNE_VERSION, createFolder, listConversations } from '@nolune/core';
import { describe, expect, it } from 'vitest';
import { createNotification } from '../../../../core/src/notifications.ts';
import { addMember, createProfile } from '../../../../core/src/profiles.ts';
import { makeFamily, makePreset, makeUser } from '../../../../core/src/test/fixtures.ts';
import * as chat from '../../routes/api/c/[id]/+server';
import * as activity from '../../routes/api/c/[id]/activity/+server';
import * as notifications from '../../routes/api/notifications/+server';
import * as chats from '../../routes/api/p/[slug]/chats/+server';
import * as folders from '../../routes/api/p/[slug]/folders/+server';
import * as folder from '../../routes/api/p/[slug]/folders/[folder]/+server';
import * as me from '../../routes/api/me/+server';
import * as newChat from '../../routes/api/p/[slug]/new-chat/+server';
import * as profiles from '../../routes/api/profiles/+server';
import * as version from '../../routes/api/version/+server';
import { decodeCursor, encodeCursor } from './api';

type Person = { id: string; name: string; isAdmin?: boolean };

/** A request as SvelteKit hands it to an endpoint, from `person` (or nobody), with a JSON body. */
function request(
	person: Person | null,
	options: { path?: string; params?: Record<string, string>; method?: string; body?: unknown } = {}
) {
	const url = new URL(options.path ?? '/api', 'http://nolune.test');
	return {
		locals: { locale: 'en', user: person && { email: '', isAdmin: false, ...person } },
		params: options.params ?? {},
		url,
		request: new Request(url, {
			method: options.method ?? 'GET',
			headers: { 'content-type': 'application/json' },
			body: options.body === undefined ? undefined : JSON.stringify(options.body)
		})
	} as never;
}

/** What the endpoint answered, its errors included, as the app would read it. */
async function answer(handler: (event: never) => Response | Promise<Response>, event: never) {
	try {
		const response = await handler(event);
		return {
			status: response.status,
			body: response.status === 204 ? null : await response.json()
		};
	} catch (err) {
		if (isHttpError(err)) return { status: err.status, body: err.body };
		throw err;
	}
}

describe('/api/version', () => {
	it('says which nolune this is to anyone, and what an app can show to someone signed in', async () => {
		expect(await answer(version.GET, request(null))).toEqual({
			status: 200,
			body: { version: NOLUNE_VERSION, api: 1 }
		});
		const { user } = makeFamily();
		expect((await answer(version.GET, request(user))).body).toEqual({
			version: NOLUNE_VERSION,
			api: 1,
			capabilities: [
				'chats',
				'notifications',
				'transcript',
				'memory',
				'automations',
				'skills',
				'profile',
				'folders',
				'images',
				'activities'
			]
		});
	});
});

describe('/api/profiles', () => {
	it('lists the profiles the person is in, with their members', async () => {
		const { user, profile } = makeFamily('Anna');
		const max = makeUser('Max');
		addMember(profile.id, 'Max');
		createProfile('Max alone', max.id);
		const { body } = await answer(profiles.GET, request(user));
		expect(body).toEqual({
			profiles: [
				{
					slug: profile.slug,
					name: 'Family',
					avatar: profile.avatar,
					members: [
						{ id: user.id, name: 'Anna', picture: null },
						{ id: max.id, name: 'Max', picture: null }
					]
				}
			]
		});
	});
});

describe('/api/p/<slug>/chats', () => {
	function family() {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const start = (body: Record<string, unknown> = {}, person: Person = user) =>
			answer(chats.POST, request(person, { method: 'POST', params: { slug: profile.slug }, body }));
		const list = (query = '', person: Person = user) =>
			answer(
				chats.GET,
				request(person, {
					path: `/api/p/${profile.slug}/chats${query}`,
					params: { slug: profile.slug }
				})
			);
		return { user, profile, preset, start, list };
	}

	it('starts a chat with the default model, and lists it', async () => {
		const { start, list, preset } = family();
		const started = await start();
		expect(started.status).toBe(201);
		expect(started.body).toMatchObject({ title: '', presetName: preset.name, running: false });
		expect((await list()).body).toEqual({ chats: [started.body], next: null });
	});

	it('pages through them with the `next` each page gives', async () => {
		const { start, list } = family();
		for (let i = 0; i < 5; i++) await start();
		const seen: string[] = [];
		let query = '?limit=2';
		for (;;) {
			const { body } = await list(query);
			seen.push(...body.chats.map((c: { id: string }) => c.id));
			if (!body.next) break;
			query = `?limit=2&after=${encodeURIComponent(body.next)}`;
		}
		expect(new Set(seen).size).toBe(5);
		expect((await list('?limit=0')).status).toBe(400);
		expect((await list('?limit=101')).status).toBe(400);
		expect((await list('?after=nonsense')).status).toBe(400);
	});

	it('starts it in a folder, with the model asked for', async () => {
		const { user, profile, start } = family();
		const other = makePreset('Opus', 'claude-opus-5');
		const folder = createFolder({ profile, name: 'Trips', userId: user.id });
		const { body } = await start({ preset: other.id, effort: 'high', folder: folder.id });
		expect(body).toMatchObject({ presetName: 'Opus', folderId: folder.id });
	});

	it("starts nothing it can't: another model, folder or command mode, or no JSON", async () => {
		const { user, profile, start } = family();
		const elsewhere = createProfile('Elsewhere', user.id);
		const theirs = createFolder({ profile: elsewhere, name: 'Theirs', userId: user.id });
		expect(await start({ preset: 'gone' })).toMatchObject({ status: 400 });
		expect(await start({ effort: 'enormous' })).toMatchObject({ status: 400 });
		expect(await start({ folder: theirs.id })).toMatchObject({ status: 400 });
		expect(await start({ commands: 'unrestricted' })).toMatchObject({ status: 403 });
		expect(await start({ uploads: [42] })).toMatchObject({ status: 400 });
		for (const body of [[], 5, 'hi', null]) {
			const event = request(user, { method: 'POST', params: { slug: profile.slug }, body });
			expect(await answer(chats.POST, event)).toMatchObject({ status: 400 });
		}
		expect(await start({ uploads: ['not-an-upload'] })).toMatchObject({ status: 400 });
		expect(listConversations(profile.id)).toEqual([]);
	});

	it("is only for the profile's members", async () => {
		const { start, list } = family();
		const stranger = makeUser('Stranger');
		expect((await list('', stranger)).status).toBe(404);
		expect((await start({}, stranger)).status).toBe(404);
		expect(await answer(chats.GET, request(null, { params: { slug: 'family' } }))).toMatchObject({
			status: 401
		});
	});
});

describe('/api/p/<slug>/new-chat', () => {
	it('says what a new chat starts with: the models, the default one, and the reasoning levels', async () => {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const { status, body } = await answer(
			newChat.GET,
			request(user, { params: { slug: profile.slug } })
		);
		expect(status).toBe(200);
		expect(body).toMatchObject({
			presets: [{ id: preset.id, name: preset.name, provider: 'anthropic' }],
			defaultPresetId: preset.id,
			efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
			commandMode: 'auto'
		});
		expect(Array.isArray(body.suggestions)).toBe(true);
		const stranger = makeUser('Stranger');
		expect(
			(await answer(newChat.GET, request(stranger, { params: { slug: profile.slug } }))).status
		).toBe(404);
	});
});

describe('/api/p/<slug>/folders', () => {
	it("lists the profile's folders", async () => {
		const { user, profile } = makeFamily();
		const folder = createFolder({ profile, name: 'Trips', userId: user.id });
		const { body } = await answer(folders.GET, request(user, { params: { slug: profile.slug } }));
		expect(body).toEqual({ folders: [{ id: folder.id, name: 'Trips' }] });
	});
});

describe('/api/p/<slug>/folders/<folder>', () => {
	it('renames a folder, answering with the name as kept, and deletes it, leaving its chats', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		const trips = createFolder({ profile, name: 'Trips', userId: user.id });
		const started = await answer(
			chats.POST,
			request(user, { method: 'POST', params: { slug: profile.slug }, body: { folder: trips.id } })
		);
		const params = { slug: profile.slug, folder: trips.id };
		expect(
			await answer(
				folder.PATCH,
				request(user, { method: 'PATCH', params, body: { name: '  Rome ' } })
			)
		).toEqual({ status: 200, body: { id: trips.id, name: 'Rome', instructions: '' } });
		expect(
			await answer(folder.PATCH, request(user, { method: 'PATCH', params, body: { name: 3 } }))
		).toMatchObject({ status: 400 });

		expect(await answer(folder.DELETE, request(user, { method: 'DELETE', params }))).toEqual({
			status: 204,
			body: null
		});
		expect(
			(await answer(folders.GET, request(user, { params: { slug: profile.slug } }))).body
		).toEqual({ folders: [] });
		expect(listConversations(profile.id).map((c) => [c.id, c.folderId])).toEqual([
			[started.body.id, null]
		]);
	});

	it("is only for the profile's own folders and members", async () => {
		const { user, profile } = makeFamily();
		const elsewhere = createProfile('Elsewhere', user.id);
		const theirs = createFolder({ profile: elsewhere, name: 'Theirs', userId: user.id });
		const stranger = makeUser('Stranger');
		const mine = createFolder({ profile, name: 'Mine', userId: user.id });
		expect(
			await answer(
				folder.DELETE,
				request(user, { method: 'DELETE', params: { slug: profile.slug, folder: theirs.id } })
			)
		).toMatchObject({ status: 404 });
		expect(
			await answer(
				folder.DELETE,
				request(stranger, { method: 'DELETE', params: { slug: profile.slug, folder: mine.id } })
			)
		).toMatchObject({ status: 404 });
	});
});

describe('/api/me', () => {
	it('says who is signed in', async () => {
		const { user } = makeFamily('Anna');
		expect(await answer(me.GET, request({ ...user, isAdmin: true }))).toEqual({
			status: 200,
			body: { id: user.id, name: 'Anna', email: '', isAdmin: true, picture: null }
		});
		expect(await answer(me.GET, request(null))).toMatchObject({ status: 401 });
	});
});

describe('/api/c/<id>', () => {
	async function chatOf() {
		const { user, profile } = makeFamily();
		makePreset();
		const started = await answer(
			chats.POST,
			request(user, { method: 'POST', params: { slug: profile.slug }, body: {} })
		);
		return { user, profile, id: started.body.id as string };
	}

	it('renames a chat, answering with the title as kept', async () => {
		const { user, id } = await chatOf();
		const rename = (title: unknown) =>
			answer(chat.PATCH, request(user, { method: 'PATCH', params: { id }, body: { title } }));
		expect(await rename('  Trip   to Rome ')).toEqual({
			status: 200,
			body: { title: 'Trip to Rome' }
		});
		expect(await rename('   ')).toMatchObject({ status: 400 });
		expect(await rename(7)).toMatchObject({ status: 400 });
	});

	it('deletes a chat, for members only', async () => {
		const { user, profile, id } = await chatOf();
		const stranger = makeUser('Stranger');
		expect(
			await answer(chat.DELETE, request(stranger, { method: 'DELETE', params: { id } }))
		).toMatchObject({
			status: 404
		});
		expect(await answer(chat.DELETE, request(user, { method: 'DELETE', params: { id } }))).toEqual({
			status: 204,
			body: null
		});
		expect(listConversations(profile.id)).toEqual([]);
	});
});

describe('/api/c/<id>/activity', () => {
	it("follows a chat for an iPhone's Live Activity, for the chat's members", async () => {
		const { user, profile } = makeFamily();
		makePreset();
		const started = await answer(
			chats.POST,
			request(user, { method: 'POST', params: { slug: profile.slug }, body: {} })
		);
		const id = started.body.id as string;
		const call = (method: string, body: unknown, person: Person = user) =>
			answer(
				method === 'POST' ? activity.POST : activity.DELETE,
				request(person, { method, params: { id }, body })
			);
		const token = 'a1'.repeat(32);
		expect((await call('POST', { token: 'not hex' })).status).toBe(400);
		expect(await call('POST', { token, sandbox: true })).toEqual({
			status: 200,
			body: { ok: true }
		});
		expect((await call('POST', { token }, makeUser('Stranger'))).status).toBe(404);
		expect((await call('DELETE', { token })).status).toBe(204);
	});
});

describe('/api/notifications', () => {
	it("lists the bell's notifications, from the person's profiles only", async () => {
		const { user, profile } = makeFamily('Anna');
		const max = makeUser('Max');
		const theirs = createProfile('Max alone', max.id);
		createNotification({ profileId: profile.id, title: 'Umbrellas', body: 'Rain at 3pm.' });
		createNotification({ profileId: theirs.id, title: 'Not for Anna', body: '' });
		const { body } = await answer(notifications.GET, request(user));
		expect(body.items.map((n: { title: string }) => n.title)).toEqual(['Umbrellas']);
		expect(body.seenAt).toBe(0);
	});
});

describe('cursors', () => {
	it('come back as they went, and nothing else reads as one', () => {
		const at = {
			updatedAt: new Date(Date.UTC(2026, 9, 2)),
			id: 'c0ffee00-1111-4222-8333-444455556666'
		};
		expect(decodeCursor(encodeCursor(at))).toEqual(at);
		for (const text of ['', 'nonsense', '12.', '.abc', '1.2.3/4'])
			expect(decodeCursor(text)).toBeNull();
	});
});
