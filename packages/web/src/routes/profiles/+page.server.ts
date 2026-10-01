import { fail, redirect } from '@sveltejs/kit';
import { createProfile, listMembers, listProfilesForUser } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireUser } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, depends }) => {
	depends('nolune:profiles');
	const user = requireUser(locals);
	const profiles = listProfilesForUser(user.id).map((p) => {
		const members = listMembers(p.id);
		return {
			slug: p.slug,
			name: p.name,
			avatar: p.avatar,
			members: members.map((m) => m.name),
			// Nobody else in it: their own profile, which the page offers to make until there is one.
			justYou: members.length === 1
		};
	});
	return { profiles, hasOwn: profiles.some((p) => p.justYou) };
};

export const actions: Actions = {
	create: async ({ locals, request }) => {
		const user = requireUser(locals);
		const form = await request.formData();
		const name = form.get('name')?.toString() ?? '';
		if (!name.trim()) {
			// Which of the page's fields to say it under.
			const from = form.get('from')?.toString();
			return fail(400, { from, message: translations(locals.locale).m.profiles.needsName });
		}
		const profile = createProfile(name, user.id);
		// The welcome: an intro, the avatar, and memories from another assistant.
		redirect(303, `/p/${profile.slug}/welcome`);
	}
};
