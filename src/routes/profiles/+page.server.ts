import { fail, redirect } from '@sveltejs/kit';
import { createProfile, listMembers, listProfilesForUser } from '@btw/core';
import { requireUser } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, depends }) => {
	depends('btw:profiles');
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
		if (!name.trim()) return fail(400, { message: 'Give the profile a name.' });
		const profile = createProfile(name, user.id);
		redirect(303, `/p/${profile.slug}`);
	}
};
