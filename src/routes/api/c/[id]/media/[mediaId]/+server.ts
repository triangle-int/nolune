import { createReadStream, statSync } from 'node:fs';
import { Readable } from 'node:stream';
import { error } from '@sveltejs/kit';
import { getMedia, mediaFile } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** `filename*` carries the real name; `filename` is an ASCII fallback for old clients. */
function contentDisposition(type: 'inline' | 'attachment', name: string): string {
	const fallback = name.replace(/[^\x20-\x7e]|["\\]/g, '_');
	const encoded = encodeURIComponent(name).replace(
		/['()*]/g,
		(c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`
	);
	return `${type}; filename="${fallback}"; filename*=UTF-8''${encoded}`;
}

/**
 * A picture or file from a reply, by the id the chat got with the message. Pictures are shown
 * inline (HEIC and TIFF through their JPEG copy); `?download` sends the original. Anything else
 * is always a download, so a copied HTML or SVG file never runs as a page of this site.
 */
export const GET: RequestHandler = ({ params, locals, url }) => {
	requireConversation(locals, params.id);
	const row = getMedia(params.id, params.mediaId);
	const file = row && mediaFile(row, url.searchParams.has('download') ? 'download' : 'view');
	if (!row || !file) error(404, 'Not found');
	let size: number;
	try {
		size = statSync(file.path).size;
	} catch {
		error(404, 'The copy of this file is gone');
	}
	return new Response(Readable.toWeb(createReadStream(file.path)) as ReadableStream, {
		headers: {
			'content-type': file.mime,
			'content-length': String(size),
			'content-disposition': contentDisposition(file.inline ? 'inline' : 'attachment', file.name),
			'x-content-type-options': 'nosniff',
			'content-security-policy': "default-src 'none'; style-src 'unsafe-inline'; sandbox",
			// A copy never changes: the same id always serves the same bytes.
			'cache-control': 'private, max-age=31536000, immutable'
		}
	});
};
