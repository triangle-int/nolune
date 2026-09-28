import { error, json } from '@sveltejs/kit';
import { continueNotification } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Turns the notification into a conversation in the sidebar and says where it lives. */
export const POST: RequestHandler = ({ locals, params }) => {
	const user = requireUser(locals);
	let target: ReturnType<typeof continueNotification>;
	try {
		target = continueNotification(params.id, user.id);
	} catch (err) {
		error(400, err instanceof Error ? err.message : String(err));
	}
	if (!target) error(404, 'Notification not found');
	return json(target);
};
