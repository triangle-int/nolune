import { json } from '@sveltejs/kit';
import { refreshSuggestions } from '@btw/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { withIcons } from '$lib/server/suggestions';
import type { RequestHandler } from './$types';

/**
 * New chips for the new-chat page, made from the profile's memory when it changed since the
 * last ones (the page asks when its load says they are stale). Waits for the model.
 */
export const GET: RequestHandler = async ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const suggestions = await refreshSuggestions(profile.slug);
	return json(withIcons(suggestions, translations(locals.locale).m), {
		headers: { 'cache-control': 'no-store' }
	});
};
