import type { Handle, ServerInit } from '@sveltejs/kit';
import { building } from '$app/environment';
import { svelteKitHandler } from 'better-auth/svelte-kit';
import { getService } from '$lib/server/service';

// Connect and migrate before the first request rather than during it.
export const init: ServerInit = async () => {
	await getService();
};

export const handle: Handle = async ({ event, resolve }) => {
	const { auth } = await getService();
	// The account pages carry a cookie, a gateway on /v1 its bearer token: getSession takes both.
	const session = await auth.api.getSession({ headers: event.request.headers });
	if (session) {
		event.locals.user = session.user;
		event.locals.session = session.session;
	}
	return svelteKitHandler({ event, resolve, auth, building });
};
