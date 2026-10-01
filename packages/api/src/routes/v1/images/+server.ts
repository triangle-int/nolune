import { notSignedIn } from '$lib/server/proxy';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** Pictures, in OpenRouter's Image API's shape: a prompt, and pictures to start from. */
export const POST: RequestHandler = async ({ locals, request }) => {
	if (!locals.user) return notSignedIn();
	const { proxy } = await getService();
	return proxy.handle({
		userId: locals.user.id,
		endpoint: 'image',
		headers: request.headers,
		body: await request.text(),
		signal: request.signal
	});
};
