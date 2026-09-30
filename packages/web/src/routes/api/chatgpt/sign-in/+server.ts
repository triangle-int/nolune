import { error, json } from '@sveltejs/kit';
import {
	PlanError,
	cancelChatGptSignIn,
	chatGptPlanStatus,
	chatGptSignInState,
	finishChatGptSignIn,
	startChatGptSignIn
} from '@nolune/core';
import { requireAdmin } from '$lib/server/access';
import type { RequestHandler } from './$types';

/*
 * Signing in with ChatGPT from a page that follows along without reloading (the welcome's model
 * step): start a sign-in, finish it with the address a browser on another device ended on, cancel
 * it, and ask where it is. Models & keys does the same through its form actions. Never the tokens.
 */

/** Where the sign-in is, and who's signed in, from what nolune keeps. */
async function state() {
	const { pending, signInError, previous } = chatGptSignInState();
	const { signedIn, problem } = await chatGptPlanStatus();
	return { pending, error: signInError, previous, signedIn, problem };
}

export const GET: RequestHandler = async ({ locals }) => {
	requireAdmin(locals);
	return json(await state());
};

/** `{ action: 'start', anotherAccount? }`, `{ action: 'finish', address }` or `{ action: 'cancel' }`. */
export const POST: RequestHandler = async ({ locals, request }) => {
	requireAdmin(locals);
	const body = (await request.json().catch(() => null)) as Record<string, unknown> | null;
	const action = body?.action;
	if (action !== 'start' && action !== 'finish' && action !== 'cancel') {
		error(400, 'Unknown action');
	}
	try {
		if (action === 'start')
			await startChatGptSignIn({ anotherAccount: body?.anotherAccount === true });
		else if (action === 'finish') await finishChatGptSignIn(String(body?.address ?? ''));
		else cancelChatGptSignIn();
	} catch (err) {
		if (!(err instanceof PlanError)) throw err;
		return json({ ...(await state()), error: err.message }, { status: 400 });
	}
	return json(await state());
};
