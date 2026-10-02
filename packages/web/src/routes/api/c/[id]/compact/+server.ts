import { error, json } from '@sveltejs/kit';
import { CompactionError, compactConversation } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Has the chat's model summarize the conversation now, for everyone who has it open; the model
 * goes on from the summary. Answers once it has started.
 */
export const POST: RequestHandler = ({ params, locals }) => {
	requireConversation(locals, params.id);
	try {
		compactConversation(params.id);
	} catch (err) {
		if (err instanceof CompactionError) error(409, err.message);
		throw err;
	}
	return json({ ok: true });
};
