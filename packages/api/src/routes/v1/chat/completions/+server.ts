import { notSignedIn } from '$lib/server/proxy';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** Chats, in OpenRouter's Chat Completions, which the gateway's openrouter.ts speaks. */
export const POST: RequestHandler = async ({ locals, request }) => {
	if (!locals.user) return notSignedIn();
	const { proxy } = await getService();
	return proxy.handle({
		userId: locals.user.id,
		endpoint: 'chat',
		headers: request.headers,
		body: await request.text(),
		signal: request.signal
	});
};
