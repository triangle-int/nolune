import { json } from '@sveltejs/kit';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { automationsOverview } from '$lib/server/automations';
import type { RequestHandler } from './$types';

/**
 * The profile's automations as their page shows them, in the person's language: a month's
 * calendar (`?month=2026-09`, this one without), and each with its schedule and last runs.
 */
export const GET: RequestHandler = ({ params, locals, url }) => {
	const { profile } = requireProfile(locals, params.slug);
	return json(
		automationsOverview(profile.id, url.searchParams.get('month'), translations(locals.locale))
	);
};
