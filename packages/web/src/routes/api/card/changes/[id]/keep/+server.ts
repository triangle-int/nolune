import { error, json } from '@sveltejs/kit';
import { MemoryError, keepOnlyInProfile, refreshMemoryLooks } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Keep only here: takes what a change put on the user's card off it, into their note in the
 * profile it came from. 409 with the reason when it can't be.
 */
export const POST: RequestHandler = ({ params, locals }) => {
	const user = requireUser(locals);
	const id = Number(params.id);
	if (!Number.isInteger(id)) error(400, 'Unknown memory change');
	try {
		const kept = keepOnlyInProfile(id, user.id);
		// Open views of the chat it came from show it undone.
		if (kept.conversationId) refreshMemoryLooks(kept.conversationId);
		return json({ ok: true, profile: kept.profile.name });
	} catch (err) {
		if (!(err instanceof MemoryError)) throw err;
		return json({ reason: err.message }, { status: 409 });
	}
};
