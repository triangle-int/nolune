import { getSnapshot, streamTranscript, subscribe } from '@nolune/core';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Server-sent events for apps of their own: the chat as `/events` gives it, with its transcript
 * built (ChatView in core) rather than its rows to build it from, and what changes in it as it
 * changes (streamTranscript).
 */
export const GET: RequestHandler = ({ params, locals, request }) => {
	requireConversation(locals, params.id);
	const encoder = new TextEncoder();
	let cleanup = () => {};

	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			const stop = streamTranscript(
				getSnapshot(params.id),
				(listener) => subscribe(params.id, listener),
				(data) => controller.enqueue(encoder.encode(`data: ${JSON.stringify(data)}\n\n`))
			);
			// Keeps proxies and the tunnel from closing an idle stream.
			const ping = setInterval(() => controller.enqueue(encoder.encode(': ping\n\n')), 15_000);
			cleanup = () => {
				stop();
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
