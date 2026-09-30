import { error, json } from '@sveltejs/kit';
import { PictureError, clearUserPicture, setUserPicture } from '@nolune/core';
import { translations } from '$lib/i18n';
import { pictureUrl } from '$lib/pictures';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * A new profile picture for the signed-in person. The body is the picture itself, which Settings
 * has already cropped to a small square; answers with where it's served from.
 */
export const PUT: RequestHandler = async ({ locals, request }) => {
	const user = requireUser(locals);
	const { m } = translations(locals.locale);
	try {
		const sha256 = setUserPicture(user.id, Buffer.from(await request.arrayBuffer()));
		return json({ picture: pictureUrl(sha256) });
	} catch (err) {
		if (!(err instanceof PictureError)) throw err;
		if (err.reason === 'tooLarge') error(413, m.account.pictureTooLarge);
		error(400, m.account.notPicture);
	}
};

/** Back to their initial. */
export const DELETE: RequestHandler = ({ locals }) => {
	clearUserPicture(requireUser(locals).id);
	return new Response(null, { status: 204 });
};
