import { error } from '@sveltejs/kit';
import { userPictureFile } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import { storedFileResponse } from '$lib/server/files';
import type { RequestHandler } from './$types';

/**
 * Someone's profile picture, for anyone signed in: the family sees each other's names already.
 * Only pictures that are someone's, never another file from the media store.
 */
export const GET: RequestHandler = ({ params, locals }) => {
	requireUser(locals);
	const file = userPictureFile(params.sha256);
	if (!file) error(404, 'Not found');
	const name = `picture.${file.mime.split('/')[1]}`;
	return storedFileResponse({ ...file, name, inline: true });
};
