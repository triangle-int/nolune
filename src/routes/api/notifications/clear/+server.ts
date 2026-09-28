import { json } from '@sveltejs/kit';
import { dismissAllNotifications } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Dismisses every notification for this user; the rest of the profile still sees them. */
export const POST: RequestHandler = ({ locals }) => {
	dismissAllNotifications(requireUser(locals).id);
	return json({ ok: true });
};
