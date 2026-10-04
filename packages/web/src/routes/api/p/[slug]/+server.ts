import { deleteProfile } from '@nolune/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Deletes the profile for everyone, as its People & profile page does: its folder goes to the trash. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { profile } = requireProfile(locals, params.slug);
	deleteProfile(profile.id);
	return new Response(null, { status: 204 });
};
