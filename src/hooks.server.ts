import { error, redirect, type Handle, type ServerInit } from '@sveltejs/kit';
import { building } from '$app/environment';
import { svelteKitHandler } from 'better-auth/svelte-kit';
import { installCliShim, recoverAfterRestart, startScheduler } from '@btw/core';
import { getAuth } from '$lib/server/auth';

export const init: ServerInit = () => {
	installCliShim();
	recoverAfterRestart();
	startScheduler();
};

/** Webhook URLs carry their own secret token instead of a login. */
const PUBLIC_PATHS = ['/login', '/api/auth/', '/api/hooks/'];

/**
 * `btw start` raises adapter-node's body limit so attachments can be uploaded. Every other route
 * keeps a small one, including the public ones that read a body before checking anything.
 */
const UPLOAD_PATH = /^\/api\/p\/[^/]+\/uploads$/;
const MAX_BODY_BYTES = 1024 * 1024;

function bodyTooLarge(request: Request, path: string): boolean {
	if (UPLOAD_PATH.test(path)) return false;
	const length = request.headers.get('content-length');
	if (length === null) return request.headers.has('transfer-encoding');
	return Number(length) > MAX_BODY_BYTES;
}

export const handle: Handle = async ({ event, resolve }) => {
	if (bodyTooLarge(event.request, event.url.pathname)) error(413, 'Request body too large');

	const auth = getAuth();
	const session = await auth.api.getSession({ headers: event.request.headers });
	if (session) {
		event.locals.session = session.session;
		event.locals.user = session.user;
	}

	const path = event.url.pathname;
	if (!event.locals.user && !PUBLIC_PATHS.some((p) => path === p || path.startsWith(p))) {
		if (path.startsWith('/api/')) error(401, 'Not signed in');
		redirect(303, '/login');
	}

	return svelteKitHandler({ event, resolve, auth, building });
};
