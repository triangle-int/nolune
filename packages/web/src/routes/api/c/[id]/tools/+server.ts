import { error, json } from '@sveltejs/kit';
import { ReloadError, reloadTools } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Reload tools: the chat gets the profile's skills and connected services as they are now, and its
 * next reply reads it all again. Answers with what changed, or null when it already had them.
 */
export const POST: RequestHandler = async ({ params, locals }) => {
	requireConversation(locals, params.id);
	try {
		return json({ changes: reloadTools(params.id) });
	} catch (err) {
		if (err instanceof ReloadError) error(409, err.message);
		throw err;
	}
};
