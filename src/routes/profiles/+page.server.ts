import { fail, redirect } from '@sveltejs/kit';
import { createProfile, listMembers, listProfilesForUser } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireUser } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, depends }) => {
	depends('nolune:profiles');
	const user = requireUser(locals);
	return {
		profiles: listProfilesForUser(user.id).map((p) => ({
			slug: p.slug,
			name: p.name,
			avatar: p.avatar,
			members: listMembers(p.id).map((m) => m.name)
		}))
	};
};

export const actions: Actions = {
	create: async ({ locals, request }) => {
		const user = requireUser(locals);
		const name = (await request.formData()).get('name')?.toString() ?? '';
		if (!name.trim()) {
			return fail(400, { message: translations(locals.locale).m.profiles.needsName });
		}
		const profile = createProfile(name, user.id);
		// The welcome: an intro, the avatar, and memories from another assistant.
		redirect(303, `/p/${profile.slug}/welcome`);
	}
};
