import { isMember, onNotificationsChanged } from '@btw/core';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Server-sent events: a "changed" ping whenever a profile the user belongs to gets a notification. */
export const GET: RequestHandler = ({ locals, request }) => {
	const user = requireUser(locals);
	const encoder = new TextEncoder();
	let cleanup = () => {};

	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			const unsubscribe = onNotificationsChanged((profileId) => {
				if (isMember(profileId, user.id)) {
					controller.enqueue(encoder.encode(`data: ${JSON.stringify({ type: 'changed' })}\n\n`));
				}
			});
			// Keeps proxies and the tunnel from closing an idle stream.
			const ping = setInterval(() => controller.enqueue(encoder.encode(': ping\n\n')), 15_000);
			cleanup = () => {
				unsubscribe();
				clearInterval(ping);
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
