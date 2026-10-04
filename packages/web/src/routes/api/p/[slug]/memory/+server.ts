import { json } from '@sveltejs/kit';
import { requireProfile } from '$lib/server/access';
import { memoryOverview } from '$lib/server/memory';
import type { RequestHandler } from './$types';

/**
 * A profile's memory as its page shows it: its notes, the members' cards, what the note-taker
 * saved lately (undone with `/<id>/undo`), whose notes are whose, and whether it learns from chats.
 */
export const GET: RequestHandler = ({ params, locals }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	return json(memoryOverview(user, profile));
};
