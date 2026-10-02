import { json } from '@sveltejs/kit';
import { listNotificationsForUser } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** The bell's notifications, newest first, and when the person last opened it (`seenAt`). */
export const GET: RequestHandler = ({ locals }) =>
	json(listNotificationsForUser(requireUser(locals).id));
