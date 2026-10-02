import { error } from '@sveltejs/kit';
import { forgetSessionPushDevice, isDeviceToken, registerPushDevice } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Registers the iPhone this session is signed in on, for the bell's notifications: nolune for iOS
 * (ios/) sends `{ token, sandbox }` from the page, with the token Apple gave it. Signing out
 * forgets it (packages/core/src/push.ts), and so does `DELETE`, which the app sends before it
 * connects to another nolune.
 */
export const POST: RequestHandler = async ({ locals, request }) => {
	const user = requireUser(locals);
	const body = (await request.json().catch(() => null)) as {
		token?: unknown;
		sandbox?: unknown;
	} | null;
	if (!locals.session || !isDeviceToken(body?.token)) error(400, 'Not a device token');
	registerPushDevice({
		token: body.token,
		sandbox: body.sandbox === true,
		userId: user.id,
		sessionId: locals.session.id
	});
	return new Response(null, { status: 204 });
};

export const DELETE: RequestHandler = ({ locals }) => {
	requireUser(locals);
	if (locals.session) forgetSessionPushDevice(locals.session.id);
	return new Response(null, { status: 204 });
};
