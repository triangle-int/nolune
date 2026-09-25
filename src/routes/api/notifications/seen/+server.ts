import { json } from '@sveltejs/kit';
import { markNotificationsSeen } from '@btw/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Opening the menu marks everything in it as read. */
export const POST: RequestHandler = ({ locals }) => {
	markNotificationsSeen(requireUser(locals).id);
	return json({ ok: true });
};
