import { json } from '@sveltejs/kit';
import { nolunePlanUsage } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Where the nolune plan's limits stand, for the bars: the whole family draws on the plan, so
 * anyone signed in sees it. Null when nolune isn't linked, or it isn't known yet (it's asked for
 * then, and /api/events says when it comes).
 */
export const GET: RequestHandler = ({ locals }) => {
	requireUser(locals);
	return json(nolunePlanUsage());
};
