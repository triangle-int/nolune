import { error, json } from '@sveltejs/kit';
import { setLearnFromChats } from '@nolune/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Whether nolune reads the profile's quiet chats over for what to remember: `{ on }`. */
export const PUT: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as { on?: unknown } | null;
	if (typeof body?.on !== 'boolean') error(400, 'Say on or off');
	setLearnFromChats(profile.id, body.on);
	return json({ learnFromChats: body.on });
};
