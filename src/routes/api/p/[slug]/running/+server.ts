import { getConversation, isMember, onRunningChange, runningConversationIds } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Server-sent events: the ids of the profile's chats btw is working in, on connect and again
 * whenever one of them starts or stops.
 */
export const GET: RequestHandler = ({ params, locals, request }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const inProfile = (id: string) => getConversation(id)?.profileId === profile.id;
	const encoder = new TextEncoder();
	let cleanup = () => {};

	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			const send = () => {
				const running = runningConversationIds().filter(inProfile);
				controller.enqueue(encoder.encode(`data: ${JSON.stringify({ running })}\n\n`));
			};
			send();
			const unsubscribe = onRunningChange((id) => {
				if (inProfile(id) && isMember(profile.id, user.id)) send();
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
