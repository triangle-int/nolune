import { error } from '@sveltejs/kit';
import { deleteUpload } from '@nolune/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Removes a file from the composer before the message is sent. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	if (!deleteUpload(profile.id, user.id, params.id)) error(404, 'Not found');
	return new Response(null, { status: 204 });
};
