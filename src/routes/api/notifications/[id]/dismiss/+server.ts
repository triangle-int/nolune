import { error, json } from '@sveltejs/kit';
import { dismissNotification } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = ({ locals, params }) => {
	if (!dismissNotification(params.id, requireUser(locals).id)) error(404, 'Notification not found');
	return json({ ok: true });
};
