import { redirect } from '@sveltejs/kit';
import { listMembers, listProfilesForUser } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import { LAST_PROFILE_COOKIE } from '$lib/server/last-profile';
import type { PageServerLoad } from './$types';

/**
 * Opening the app goes straight to a chat, like ChatGPT: the last profile, or the only one if it's
 * their own. Someone who is only in profiles others made sees the profiles first, with how they
 * work and an offer of one of their own; once they open one, it's the last profile.
 */
export const load: PageServerLoad = ({ locals, cookies }) => {
	const user = requireUser(locals);
	const profiles = listProfilesForUser(user.id);
	const last = profiles.find((p) => p.slug === cookies.get(LAST_PROFILE_COOKIE));
	const only = profiles.length === 1 ? profiles[0] : undefined;
	const target = last ?? (only && listMembers(only.id).length === 1 ? only : undefined);
	redirect(303, target ? `/p/${target.slug}` : '/profiles');
};
