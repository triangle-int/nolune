import { json } from '@sveltejs/kit';
import { kick } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Retries after an error: runs the agent if the last message is still unanswered. */
export const POST: RequestHandler = ({ params, locals }) => {
	requireConversation(locals, params.id);
	kick(params.id);
	return json({ ok: true });
};
