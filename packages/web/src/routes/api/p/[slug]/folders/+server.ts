import { error, json } from '@sveltejs/kit';
import { FolderError, createFolder } from '@nolune/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Creates a folder of chats in the profile. */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as { name?: unknown } | null;
	try {
		const created = createFolder({ profile, name: String(body?.name ?? ''), userId: user.id });
		return json({ id: created.id, name: created.name });
	} catch (err) {
		if (err instanceof FolderError) error(400, err.message);
		throw err;
	}
};
