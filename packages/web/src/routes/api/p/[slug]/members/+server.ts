import { error, json } from '@sveltejs/kit';
import { addMemberWithNote } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Adds someone with an account on this nolune to the profile: `{ who }`, their name or email, and
 * `note`, which people note is theirs (a path, or `new`). Memory may know them already: without a
 * note, that answers `{ choose: { who, candidates } }` and adds nobody, to ask which is theirs.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as { who?: unknown; note?: unknown } | null;
	if (typeof body?.who !== 'string') error(400, 'Say who');
	const note = typeof body.note === 'string' && body.note ? body.note : undefined;
	try {
		const result = addMemberWithNote(profile, body.who, note);
		if (!result.added) return json({ choose: { who: result.name, candidates: result.candidates } });
		return json({ message: translations(locals.locale).m.profile.added(result.name) });
	} catch (err) {
		if (err instanceof Error) error(400, err.message);
		throw err;
	}
};
