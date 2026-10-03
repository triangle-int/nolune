import { error, json } from '@sveltejs/kit';
import { TitleError, getConversation, renameConversation } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import { removeChat } from '$lib/server/chats';
import type { RequestHandler } from './$types';

/** Renames the chat for everyone, and answers with the title as kept. */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { title?: unknown } | null;
	if (typeof body?.title !== 'string') error(400, 'Send JSON with a title');
	try {
		renameConversation(params.id, body.title);
	} catch (err) {
		if (err instanceof TitleError) error(400, err.message);
		throw err;
	}
	return json({ title: getConversation(params.id)?.title ?? '' });
};

/** Deletes the chat for everyone, as its page's menu does. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { user } = requireConversation(locals, params.id);
	removeChat(params.id, user.name);
	return new Response(null, { status: 204 });
};
