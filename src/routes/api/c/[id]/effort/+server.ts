import { error, json } from '@sveltejs/kit';
import { EFFORTS, setEffort, type Effort } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { effort?: unknown } | null;
	const effort = body?.effort as Effort;
	if (!EFFORTS.includes(effort)) error(400, 'Unknown reasoning level');
	setEffort(params.id, effort);
	return json({ ok: true });
};
