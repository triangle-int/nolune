import { json } from '@sveltejs/kit';
import { refreshSuggestions } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import { withIcons } from '$lib/server/suggestions';
import type { RequestHandler } from './$types';

/**
 * New chips for the person on the new-chat page, made from the profile's memory when it changed
 * since their last ones (the page asks when its load says they are stale). Waits for the model.
 */
export const GET: RequestHandler = async ({ locals, params }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const person = { id: user.id, name: user.name };
	return json(withIcons(await refreshSuggestions(profile.slug, person)), {
		headers: { 'cache-control': 'no-store' }
	});
};
