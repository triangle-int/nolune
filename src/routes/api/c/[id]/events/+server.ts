import { getSnapshot, subscribe, type LiveEvent } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Server-sent events: a full snapshot on connect, then live updates. */
export const GET: RequestHandler = ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const encoder = new TextEncoder();
	let cleanup = () => {};

	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			const send = (data: LiveEvent | { type: 'snapshot'; snapshot: unknown }) =>
				controller.enqueue(encoder.encode(`data: ${JSON.stringify(data)}\n\n`));
			send({ type: 'snapshot', snapshot: getSnapshot(params.id) });
			const unsubscribe = subscribe(params.id, send);
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
