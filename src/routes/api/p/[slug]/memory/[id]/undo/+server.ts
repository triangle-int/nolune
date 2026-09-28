import { error, json } from '@sveltejs/kit';
import { MemoryUndoError, refreshMemoryLooks, undoMemoryChange } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Undoes one of the note-taker's memory changes, from its chat or the Memory page. 409 with the
 * reason when it can't be: it already was, or the note changed since.
 */
export const POST: RequestHandler = ({ params, locals }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const id = Number(params.id);
	if (!Number.isInteger(id)) error(400, 'Unknown memory change');
	let conversationId: string | null;
	try {
		({ conversationId } = undoMemoryChange(profile, id, user.name));
	} catch (err) {
		if (!(err instanceof MemoryUndoError)) throw err;
		if (err.reason === 'missing') error(404, 'Unknown memory change');
		return json({ reason: err.reason }, { status: 409 });
	}
	// Open views of the chat it came from show it undone.
	if (conversationId) refreshMemoryLooks(conversationId);
	return json({ ok: true });
};
