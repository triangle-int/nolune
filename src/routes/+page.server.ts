import { redirect } from '@sveltejs/kit';
import { listProfilesForUser } from '@btw/core';
import { requireUser } from '$lib/server/access';
import { LAST_PROFILE_COOKIE } from '$lib/server/last-profile';
import type { PageServerLoad } from './$types';

/** Opening the app goes straight to a chat, like ChatGPT: the last profile, or the only one. */
export const load: PageServerLoad = ({ locals, cookies }) => {
	const user = requireUser(locals);
	const profiles = listProfilesForUser(user.id);
	const last = profiles.find((p) => p.slug === cookies.get(LAST_PROFILE_COOKIE));
	const target = last ?? (profiles.length === 1 ? profiles[0] : undefined);
	redirect(303, target ? `/p/${target.slug}` : '/profiles');
};
