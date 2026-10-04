import { error, json } from '@sveltejs/kit';
import { followLiveActivity, forgetLiveActivity, isDeviceToken } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * A Live Activity on an iPhone for the reply nolune is writing in the chat (nolune for iOS):
 * `{ token, sandbox }`, the activity's token from Apple. The gateway sends what nolune does to it
 * through the relay until the reply is done (live-activities.ts in core).
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as {
		token?: unknown;
		sandbox?: unknown;
	} | null;
	if (!isDeviceToken(body?.token)) error(400, 'Send JSON with the activity’s token');
	followLiveActivity(params.id, { token: body.token, sandbox: body.sandbox === true });
	return json({ ok: true });
};

/** The person ended the activity: `{ token }`. */
export const DELETE: RequestHandler = async ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const body = (await request.json().catch(() => null)) as { token?: unknown } | null;
	if (isDeviceToken(body?.token)) forgetLiveActivity(params.id, body.token);
	return new Response(null, { status: 204 });
};
