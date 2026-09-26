import { error, json } from '@sveltejs/kit';
import { isSubagentConversation, kick } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Retries after an error: runs the agent if the last message is still unanswered. */
export const POST: RequestHandler = ({ params, locals }) => {
	requireConversation(locals, params.id);
	// A subagent works for the agent that started it, which gives it more work if it should retry.
	if (isSubagentConversation(params.id))
		error(403, 'Only the agent that started it runs a subagent.');
	kick(params.id);
	return json({ ok: true });
};
