import { isHttpError } from '@sveltejs/kit';
import { createFolder, createTrigger, getTrigger } from '@nolune/core';
import { describe, expect, it } from 'vitest';
import { addMember } from '../../../../core/src/profiles.ts';
import { makeFamily, makePreset, makeUser } from '../../../../core/src/test/fixtures.ts';
import * as automation from '../../routes/api/p/[slug]/automations/[id]/+server';
import * as automationRun from '../../routes/api/p/[slug]/automations/[id]/run/+server';
import * as automations from '../../routes/api/p/[slug]/automations/+server';
import * as folder from '../../routes/api/p/[slug]/folders/[folder]/+server';
import * as images from '../../routes/api/p/[slug]/images/+server';
import * as member from '../../routes/api/p/[slug]/members/[user]/+server';
import * as members from '../../routes/api/p/[slug]/members/+server';
import * as learning from '../../routes/api/p/[slug]/memory/learning/+server';
import * as memory from '../../routes/api/p/[slug]/memory/+server';
import * as notes from '../../routes/api/p/[slug]/memory/notes/+server';
import * as settings from '../../routes/api/p/[slug]/settings/+server';
import * as skills from '../../routes/api/p/[slug]/skills/+server';
import * as profileRoute from '../../routes/api/p/[slug]/+server';

/*
 * The JSON the iOS app's screens read for a profile's pages (#137): what each page loads, and
 * what its forms do, for the profile's members only.
 */

type Person = { id: string; name: string; isAdmin?: boolean };

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

describe('/api/p/<slug>/skills', () => {
	it('lists the skills, and turns them off and on', async () => {
		const { user, profile } = makeFamily();
		const params = { slug: profile.slug };
		const { body } = await answer(skills.GET, request(user, { params }));
		expect(Array.isArray(body.skills)).toBe(true);
		const first = body.skills[0] as { name: string; enabled: boolean } | undefined;
		if (first) {
			const off = await answer(
				skills.PATCH,
				request(user, { method: 'PATCH', params, body: { names: [first.name], enabled: false } })
			);
			expect(off.body.skills.find((s: { name: string }) => s.name === first.name).enabled).toBe(
				false
			);
		}
		const none = await answer(
			skills.PATCH,
			request(user, { method: 'PATCH', params, body: { names: ['no-such-skill'], enabled: false } })
		);
		expect(none.status).toBe(400);
		expect((await answer(skills.GET, request(makeUser('Stranger'), { params }))).status).toBe(404);
	});
});

describe('/api/p/<slug>/folders/<folder>', () => {
	it('shows a folder with its instructions and files, and sets them', async () => {
		const { user, profile } = makeFamily();
		const made = createFolder({ profile, name: 'Trips', userId: user.id });
		const params = { slug: profile.slug, folder: made.id };
		expect((await answer(folder.GET, request(user, { params }))).body).toMatchObject({
			id: made.id,
			name: 'Trips',
			instructions: '',
			files: [],
			maxFiles: 50
		});
		const set = await answer(
			folder.PATCH,
			request(user, { method: 'PATCH', params, body: { instructions: 'Book trains early.' } })
		);
		expect(set.body).toEqual({ id: made.id, name: 'Trips', instructions: 'Book trains early.' });
		expect(
			(await answer(folder.PATCH, request(user, { method: 'PATCH', params, body: {} }))).status
		).toBe(400);
	});
});

describe('/api/p/<slug>/memory', () => {
	it('shows the memory, writes and forgets a note, and learns from chats or not', async () => {
		const { user, profile } = makeFamily();
		const params = { slug: profile.slug };
		const first = await answer(memory.GET, request(user, { params }));
		expect(first.body).toMatchObject({
			core: { path: 'core.md' },
			learnFromChats: expect.any(Boolean)
		});

		const write = (text: string, basedOn = 0) =>
			answer(
				notes.PUT,
				request(user, { method: 'PUT', params, body: { path: 'home.md', text, basedOn } })
			);
		expect((await write('# Home\n\n- The wifi is "smiths".')).status).toBe(200);
		const after = await answer(memory.GET, request(user, { params }));
		const home = after.body.files.find((f: { path: string }) => f.path === 'home.md');
		expect(home.text).toContain('wifi');
		// Written since it was opened: a conflict.
		expect((await write('Other', home.updatedAt - 1000)).status).toBe(409);
		expect((await write('   ')).status).toBe(400);
		expect(
			(
				await answer(
					notes.DELETE,
					request(user, { method: 'DELETE', params, body: { path: 'home.md' } })
				)
			).status
		).toBe(204);

		const off = await answer(
			learning.PUT,
			request(user, { method: 'PUT', params, body: { on: false } })
		);
		expect(off.body).toEqual({ learnFromChats: false });
	});
});

describe('/api/p/<slug>/automations', () => {
	it('lists the automations, and changes, runs and deletes one of the profile’s', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		const t = createTrigger({
			profileId: profile.id,
			name: 'Weather',
			when: { kind: 'cron', cron: '30 7 * * 1-5' },
			what: { action: 'agent', prompt: 'Check the weather.' }
		});
		const params = { slug: profile.slug, id: t.id };
		const { body } = await answer(automations.GET, request(user, { params }));
		expect(body.triggers).toMatchObject([
			{ id: t.id, name: 'Weather', state: 'on', text: 'Check the weather.' }
		]);
		expect(body.calendar.cells.length).toBeGreaterThan(27);

		const paused = await answer(
			automation.PATCH,
			request(user, { method: 'PATCH', params, body: { enabled: false } })
		);
		expect(paused.status).toBe(200);
		expect(getTrigger(t.id)?.enabled).toBe(false);
		await answer(
			automation.PATCH,
			request(user, {
				method: 'PATCH',
				params,
				body: { summary: 'Mornings', text: 'Look outside.' }
			})
		);
		expect(getTrigger(t.id)).toMatchObject({ summary: 'Mornings', prompt: 'Look outside.' });

		const stranger = makeUser('Stranger');
		expect(
			(await answer(automationRun.POST, request(stranger, { method: 'POST', params }))).status
		).toBe(404);
		expect(
			(await answer(automation.DELETE, request(user, { method: 'DELETE', params }))).status
		).toBe(200);
		expect(getTrigger(t.id)).toBeFalsy();
	});
});

describe('/api/p/<slug>/settings and members', () => {
	it('renames the profile, picks its avatar, adds and removes members, and deletes it', async () => {
		const { user, profile } = makeFamily('Anna');
		const max = makeUser('Max');
		const params = { slug: profile.slug };
		const patch = (body: unknown) =>
			answer(settings.PATCH, request(user, { method: 'PATCH', params, body }));
		expect((await patch({ name: 'The Smiths' })).status).toBe(200);
		expect((await patch({ avatar: 'moon' })).status).toBe(200);
		expect((await patch({ avatar: 'dragon' })).status).toBe(400);
		const shown = await answer(settings.GET, request(user, { params }));
		expect(shown.body).toMatchObject({ name: 'The Smiths', avatar: 'moon', others: ['Max'] });

		const added = await answer(
			members.POST,
			request(user, { method: 'POST', params, body: { who: 'Max', note: 'new' } })
		);
		expect(added.status).toBe(200);
		const removed = await answer(
			member.DELETE,
			request(user, { method: 'DELETE', params: { ...params, user: max.id } })
		);
		expect(removed.body).toMatchObject({ left: false });
		addMember(profile.id, 'Max');
		const left = await answer(
			member.DELETE,
			request(user, { method: 'DELETE', params: { ...params, user: user.id } })
		);
		expect(left.body).toMatchObject({ left: true });
		expect((await answer(settings.GET, request(user, { params }))).status).toBe(404);
		expect(
			(await answer(profileRoute.DELETE, request(max, { method: 'DELETE', params }))).status
		).toBe(204);
	});
});

describe('/api/p/<slug>/images', () => {
	it('lists the picture templates, and needs a description without one', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		const params = { slug: profile.slug };
		const { body } = await answer(images.GET, request(user, { params }));
		expect(Array.isArray(body.templates)).toBe(true);
		expect(typeof body.ready).toBe('boolean');
		const empty = await answer(
			images.POST,
			request(user, { method: 'POST', params, body: { text: ' ' } })
		);
		expect(empty.status).toBe(400);
	});
});
