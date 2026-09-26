import { error, json } from '@sveltejs/kit';
import { FolderError, moveConversation } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Moves the chat into a folder of its profile, or out of any with `null`. Its system prompt is
 * built again at the start of its next turn.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { folderId?: unknown } | null;
	const folderId = body?.folderId;
	if (folderId !== null && typeof folderId !== 'string') error(400, 'Pick a folder');
	try {
		moveConversation(profile.id, params.id, folderId);
	} catch (err) {
		if (err instanceof FolderError) error(404, err.message);
		throw err;
	}
	return json({ ok: true });
};
