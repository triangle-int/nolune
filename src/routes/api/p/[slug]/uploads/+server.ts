import { Readable } from 'node:stream';
import type { ReadableStream as WebReadableStream } from 'node:stream/web';
import { error, json } from '@sveltejs/kit';
import { MAX_MEDIA_BYTES, TooLargeError, createUpload } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

const TOO_LARGE = `Larger than ${MAX_MEDIA_BYTES / (1024 * 1024)} MB`;

/**
 * A file attached in the composer, before the message is sent. The body is the file itself and
 * `x-file-name` its URL-encoded name. Answers with the id to send the message with.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	if (Number(request.headers.get('content-length')) > MAX_MEDIA_BYTES) error(413, TOO_LARGE);
	if (!request.body) error(400, 'No file');
	let name = 'file';
	try {
		name = decodeURIComponent(request.headers.get('x-file-name') ?? '') || name;
	} catch {
		// keep the default
	}
	try {
		const row = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name,
			body: Readable.fromWeb(request.body as WebReadableStream),
			signal: request.signal
		});
		return json({ id: row.id, name: row.name, mime: row.mime, bytes: row.bytes });
	} catch (err) {
		if (err instanceof TooLargeError) error(413, TOO_LARGE);
		throw err;
	}
};
