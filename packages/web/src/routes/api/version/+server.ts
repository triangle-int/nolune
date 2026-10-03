import { json } from '@sveltejs/kit';
import { NOLUNE_VERSION } from '@nolune/core';
import { API_LEVEL, CAPABILITIES } from '$lib/server/api';
import type { RequestHandler } from './$types';

/**
 * Which nolune this is, for apps of their own before and after signing in: its version, the level
 * of the JSON it serves them (lib/server/api.ts), and signed in, what they can show natively.
 */
export const GET: RequestHandler = ({ locals }) =>
	json({
		version: NOLUNE_VERSION,
		api: API_LEVEL,
		...(locals.user ? { capabilities: [...CAPABILITIES] } : {})
	});
