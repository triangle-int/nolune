import { error, json } from '@sveltejs/kit';
import { ModelSwitchError, changeModel } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Switches the chat to another model preset; everyone who has it open sees the change. */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { presetId?: unknown } | null;
	if (typeof body?.presetId !== 'string') error(400, 'Pick a model');
	try {
		return json(changeModel(params.id, body.presetId));
	} catch (err) {
		if (err instanceof ModelSwitchError) error(400, err.message);
		throw err;
	}
};
