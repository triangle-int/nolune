import { error, json } from '@sveltejs/kit';
import { MemoryError, linkPersonNote, membersWithNotes, removeMember } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Says which people note is a member's: `{ note }`, a path, or `new`. */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as { note?: unknown } | null;
	const member = membersWithNotes(profile).find((m) => m.id === params.user);
	if (!member) error(404, 'Not a member');
	if (typeof body?.note !== 'string' || !body.note) error(400, 'Choose a note.');
	try {
		const path = linkPersonNote(profile, member.id, body.note);
		return json({
			note: path,
			message: translations(locals.locale).m.profile.linked(member.name, path)
		});
	} catch (err) {
		if (err instanceof MemoryError) error(400, err.message);
		throw err;
	}
};

/** Takes someone out of the profile; themselves, it's leaving it. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	removeMember(profile.id, params.user);
	return json({
		left: params.user === user.id,
		message: translations(locals.locale).m.profile.removed
	});
};
