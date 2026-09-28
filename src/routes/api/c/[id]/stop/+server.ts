import { json } from '@sveltejs/kit';
import { stopConversation } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = ({ params, locals }) => {
	const { user } = requireConversation(locals, params.id);
	stopConversation(params.id, user.name);
	return json({ ok: true });
};
