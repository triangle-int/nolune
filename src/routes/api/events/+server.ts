import { isMember, onNotificationsChanged, onProfileChanged } from '@nolune/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Server-sent events for what other people and nolune change under a signed-in page: a ping when a
 * profile the user belongs to gets a notification (`notifications`), or is renamed or given another
 * avatar (`profiles`). The page reloads the data that depends on it. One stream for both, as each
 * open tab also keeps its chat's stream and browsers allow few connections per site.
 */
export const GET: RequestHandler = ({ locals, request }) => {
	const user = requireUser(locals);
	const encoder = new TextEncoder();
	let cleanup = () => {};

	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			const ping = (type: 'notifications' | 'profiles') => (profileId: string) => {
				if (isMember(profileId, user.id)) {
					controller.enqueue(encoder.encode(`data: ${JSON.stringify({ type })}\n\n`));
				}
			};
			const stopNotifications = onNotificationsChanged(ping('notifications'));
			const stopProfiles = onProfileChanged(ping('profiles'));
			// Keeps proxies and the tunnel from closing an idle stream.
			const keepAlive = setInterval(() => controller.enqueue(encoder.encode(': ping\n\n')), 15_000);
			cleanup = () => {
				stopNotifications();
				stopProfiles();
				clearInterval(keepAlive);
			};
			request.signal.addEventListener('abort', () => {
				cleanup();
				try {
					controller.close();
				} catch {
					// already closed
				}
			});
		},
		cancel() {
			cleanup();
		}
	});

	return new Response(stream, {
		headers: {
			'content-type': 'text/event-stream',
			'cache-control': 'no-cache, no-transform',
			'x-accel-buffering': 'no'
		}
	});
};
