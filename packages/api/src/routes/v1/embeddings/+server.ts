import { notSignedIn } from '$lib/server/proxy';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** Embeddings for memory search, in OpenAI's shape. */
export const POST: RequestHandler = async ({ locals, request }) => {
	if (!locals.user) return notSignedIn();
	const { proxy } = await getService();
	return proxy.handle({
		userId: locals.user.id,
		endpoint: 'embedding',
		headers: request.headers,
		body: await request.text(),
		signal: request.signal
	});
};
