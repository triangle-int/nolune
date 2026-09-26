import { error, json } from '@sveltejs/kit';
import { AttachmentError, SubagentError, sendMessage } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** `{text, uploads}`: `uploads` are the ids of files attached in the composer, in order. */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user } = requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as {
		text?: unknown;
		uploads?: unknown;
	} | null;
	const text = typeof body?.text === 'string' ? body.text : '';
	const uploads = Array.isArray(body?.uploads)
		? body.uploads.filter((id): id is string => typeof id === 'string')
		: [];
	if (!text.trim() && !uploads.length) error(400, 'Message is empty');
	try {
		await sendMessage(params.id, { id: user.id, name: user.name }, text, uploads);
	} catch (err) {
		if (err instanceof AttachmentError) error(400, err.message);
		if (err instanceof SubagentError) error(403, err.message);
		throw err;
	}
	return json({ ok: true });
};
