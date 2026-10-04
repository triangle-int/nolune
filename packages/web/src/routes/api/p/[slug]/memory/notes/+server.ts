import { error, json } from '@sveltejs/kit';
import { MemoryConflictError, MemoryError, forgetMemoryFile, writeMemoryFile } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Writes a note, as the memory page's editor does: `{ path, text, basedOn }`, `basedOn` being the
 * note's `updatedAt` when it was opened (0 for one that isn't there yet). 409 when it changed since.
 */
export const PUT: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const { m } = translations(locals.locale);
	const body = (await request.json().catch(() => null)) as {
		path?: unknown;
		text?: unknown;
		basedOn?: unknown;
	} | null;
	if (typeof body?.path !== 'string' || typeof body.text !== 'string') error(400, 'Send a note');
	if (!body.text.trim()) error(400, m.memory.empty);
	try {
		writeMemoryFile(profile.slug, body.path, body.text, Number(body.basedOn ?? 0));
	} catch (err) {
		if (err instanceof MemoryConflictError) error(409, m.memory.conflict(err.message));
		if (err instanceof MemoryError) error(400, err.message);
		throw err;
	}
	return json({ message: m.memory.saved });
};

/** Forgets a note: `{ path }`. */
export const DELETE: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as { path?: unknown } | null;
	if (typeof body?.path !== 'string') error(400, 'Name the note');
	try {
		forgetMemoryFile(profile.slug, body.path);
	} catch (err) {
		if (err instanceof MemoryError) error(400, err.message);
		throw err;
	}
	return new Response(null, { status: 204 });
};
