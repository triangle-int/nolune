import { error } from '@sveltejs/kit';
import { getMedia, mediaFile } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import { storedFileResponse } from '$lib/server/files';
import type { RequestHandler } from './$types';

/**
 * A picture or file from a reply, by the id the chat got with the message. Pictures are shown
 * inline (HEIC and TIFF through their JPEG copy); `?download` sends the original. Anything else
 * is always a download.
 */
export const GET: RequestHandler = ({ params, locals, url }) => {
	requireConversation(locals, params.id);
	const row = getMedia(params.id, params.mediaId);
	const file = row && mediaFile(row, url.searchParams.has('download') ? 'download' : 'view');
	if (!row || !file) error(404, 'Not found');
	return storedFileResponse(file);
};
