import { error, json } from '@sveltejs/kit';
import { MemoryConflictError, MemoryError, mergeProfileNotes } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Moves a note to another path, or into one that's there already, which merges them: `{ from, to }`.
 * Whose note it is, and its history, go with it.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const { m } = translations(locals.locale);
	const body = (await request.json().catch(() => null)) as { from?: unknown; to?: unknown } | null;
	if (typeof body?.from !== 'string' || typeof body.to !== 'string') error(400, 'Send from and to');
	try {
		const moved = mergeProfileNotes(profile, body.from, body.to);
		const say = moved.merged ? m.memory.move.merged : m.memory.move.moved;
		return json({ path: moved.into, merged: moved.merged, message: say(moved.from, moved.into) });
	} catch (err) {
		if (err instanceof MemoryError || err instanceof MemoryConflictError) error(400, err.message);
		throw err;
	}
};
