import { error, json } from '@sveltejs/kit';
import {
	PlanError,
	cancelNolunePlanSignIn,
	nolunePlanAccountUrl,
	nolunePlanSignInState,
	nolunePlanStatus,
	startNolunePlanSignIn
} from '@nolune/core';
import { requireAdmin } from '$lib/server/access';
import type { RequestHandler } from './$types';

/*
 * Linking nolune to the nolune plan from a page that follows along without reloading (the
 * welcome's model step): start a link, cancel it, and ask where it is. Models & keys does the
 * same through its form actions. Never the token.
 */

/**
 * Where the link is, and who nolune is linked as, from what nolune keeps. With `check`, a linked
 * nolune asks the API too, which says whether the account has a plan yet.
 */
async function state(check = false) {
	const { pending, signInError } = nolunePlanSignInState();
	const { signedIn, problem, noPlan } = await nolunePlanStatus({ check: check && !pending });
	return {
		pending,
		error: signInError,
		signedIn,
		problem,
		noPlan,
		accountUrl: nolunePlanAccountUrl()
	};
}

/** `?check=1` asks the API whether the account has a plan (the welcome, waiting on one). */
export const GET: RequestHandler = async ({ locals, url }) => {
	requireAdmin(locals);
	return json(await state(url.searchParams.get('check') === '1'));
};

/** `{ action: 'start' }` or `{ action: 'cancel' }`. */
export const POST: RequestHandler = async ({ locals, request }) => {
	requireAdmin(locals);
	const body = (await request.json().catch(() => null)) as Record<string, unknown> | null;
	const action = body?.action;
	if (action !== 'start' && action !== 'cancel') error(400, 'Unknown action');
	try {
		if (action === 'start') await startNolunePlanSignIn();
		else cancelNolunePlanSignIn();
	} catch (err) {
		if (!(err instanceof PlanError)) throw err;
		return json({ ...(await state()), error: err.message }, { status: 400 });
	}
	return json(await state());
};
