import { error, json } from '@sveltejs/kit';
import { sendMessage } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user } = requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { text?: unknown } | null;
	const text = typeof body?.text === 'string' ? body.text : '';
	if (!text.trim()) error(400, 'Message is empty');
	sendMessage(params.id, { id: user.id, name: user.name }, text);
	return json({ ok: true });
};
