import { readFileSync } from 'node:fs';
import { error } from '@sveltejs/kit';
import { inspectImage, profileImageTemplatesDir, scanImageTemplates } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** A template's cover picture (cover.png, .jpg or .webp next to its TEMPLATE.md). */
export const GET: RequestHandler = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const { templates } = scanImageTemplates(profileImageTemplatesDir(profile.slug));
	const template = templates.find((t) => t.id === params.id);
	if (!template?.cover) error(404, 'No cover');
	let data: Buffer;
	try {
		data = readFileSync(template.cover);
	} catch {
		error(404, 'No cover');
	}
	// Served by what the file is, not its name, and only if it's a picture browsers show.
	const info = inspectImage(data);
	if (!info) error(404, 'No cover');
	return new Response(new Uint8Array(data), {
		headers: {
			'content-type': info.mediaType,
			'x-content-type-options': 'nosniff',
			'content-security-policy': "default-src 'none'; sandbox",
			// The page links it with its modification time, so a new cover gets a new URL.
			'cache-control': 'private, max-age=31536000, immutable'
		}
	});
};
