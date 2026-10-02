import { error, redirect, type Handle, type ServerInit } from '@sveltejs/kit';
import { building } from '$app/environment';
import { svelteKitHandler } from 'better-auth/svelte-kit';
import { serveGatewayCommands } from '@nolune/cli/serve';
import {
	holdMcpConnections,
	installCliShim,
	recoverAfterRestart,
	startScheduler,
	startUpdateChecks
} from '@nolune/core';
import { matchLocale, translations } from '$lib/i18n';
import { PREFERENCES_COOKIE, parsePreferences } from '$lib/preferences.svelte';
import { getAuth } from '$lib/server/auth';

export const init: ServerInit = () => {
	installCliShim();
	recoverAfterRestart();
	startScheduler();
	// Whether there's a newer nolune, for admins (packages/core/src/updates.ts).
	startUpdateChecks();
	// The agent's `nolune` commands run here, on the nolune already loaded (packages/cli/src/serve.ts).
	serveGatewayCommands();
	// So `nolune mcp` keeps each MCP server's connection open between them (packages/core/src/mcp.ts).
	holdMcpConnections();
};

/**
 * Webhook URLs and invite links carry their own secret token instead of a login. Apps ask which
 * nolune this is (`/api/version`) before anyone signs in.
 */
const PUBLIC_PATHS = ['/login', '/api/auth/', '/api/hooks/', '/invite/', '/api/version'];

/**
 * `nolune start` raises adapter-node's body limit so attachments can be uploaded. Every other route
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
	// The interface's language: the one picked in Settings, else the browser's.
	const { language } = parsePreferences(event.cookies.get(PREFERENCES_COOKIE));
	const locale =
		language === 'auto' ? matchLocale(event.request.headers.get('accept-language')) : language;
	event.locals.locale = locale;

	if (bodyTooLarge(event.request, event.url.pathname)) error(413, 'Request body too large');

	const auth = getAuth();
	const session = await auth.api.getSession({ headers: event.request.headers });
	if (session) {
		event.locals.session = session.session;
		event.locals.user = session.user;
	}

	const path = event.url.pathname;
	if (!event.locals.user && !PUBLIC_PATHS.some((p) => path === p || path.startsWith(p))) {
		if (path.startsWith('/api/')) error(401, translations(locale).m.errors.notSignedIn);
		redirect(303, '/login');
	}

	return svelteKitHandler({
		event,
		resolve: (event) =>
			resolve(event, { transformPageChunk: ({ html }) => html.replace('%nolune.lang%', locale) }),
		auth,
		building
	});
};
