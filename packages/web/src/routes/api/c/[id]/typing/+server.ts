import { error, json } from '@sveltejs/kit';
import { setTyping } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** `{typing}`: whether the signed-in person is writing in the chat, for the others who have it open. */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user } = requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { typing?: unknown } | null;
	if (typeof body?.typing !== 'boolean') error(400, 'Expected {typing: boolean}');
	setTyping(params.id, { id: user.id, name: user.name }, body.typing);
	return json({ ok: true });
};
