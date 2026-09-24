import { json } from '@sveltejs/kit';
import { stop } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = ({ params, locals }) => {
	const { user } = requireConversation(locals, params.id);
	stop(params.id, user.name);
	return json({ ok: true });
};
