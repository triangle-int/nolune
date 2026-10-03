import { error } from '@sveltejs/kit';
import { getFolder, getFolderFile, mediaFile, removeFolderFile } from '@nolune/core';
import { requireProfile } from '$lib/server/access';
import { storedFileResponse } from '$lib/server/files';
import type { RequestHandler } from './$types';

/**
 * A file of a folder, from the media store rather than the copy the agent works with (which it
 * may change or move). Like pictures in replies: shown inline when a browser can, `?download`
 * for the original.
 */
export const GET: RequestHandler = ({ params, locals, url }) => {
	const { profile } = requireProfile(locals, params.slug);
	const row = getFolder(profile.id, params.folder) && getFolderFile(params.folder, params.file);
	if (!row) error(404, 'Not found');
	const file = mediaFile(
		{ ...row, status: 'ok' },
		url.searchParams.has('download') ? 'download' : 'view'
	);
	if (!file) error(404, 'Not found');
	return storedFileResponse(file);
};

/** Takes a file out of the folder, as its page does. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { profile } = requireProfile(locals, params.slug);
	if (!getFolder(profile.id, params.folder)) error(404, 'Not found');
	removeFolderFile(params.folder, params.file);
	return new Response(null, { status: 204 });
};
