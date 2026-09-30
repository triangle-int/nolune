import { createReadStream, statSync } from 'node:fs';
import { Readable } from 'node:stream';
import { error } from '@sveltejs/kit';

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
 * A copy from the media store. Anything that isn't `inline` is a download, so a copied HTML or
 * SVG file never runs as a page of this site. A copy never changes: the same id always serves
 * the same bytes.
 */
export function storedFileResponse(file: {
	path: string;
	mime: string;
	name: string;
	inline: boolean;
}): Response {
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
			'cache-control': 'private, max-age=31536000, immutable'
		}
	});
}
