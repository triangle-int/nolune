import { error, json } from '@sveltejs/kit';
import { describeApiError, isProvider, listModels } from '@btw/core';
import { requireAdmin } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** The models a provider offers, for the admin page's model picker. */
export const GET: RequestHandler = async ({ locals, url }) => {
	requireAdmin(locals);
	const provider = url.searchParams.get('provider') ?? '';
	if (!isProvider(provider)) error(400, 'Unknown provider');
	try {
		return json({ models: await listModels(provider), problem: null });
	} catch (err) {
		// The picker still takes a typed id, so this is a note under it rather than an error.
		return json({ models: [], problem: describeApiError(err) });
	}
};
