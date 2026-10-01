import { json } from '@sveltejs/kit';
import { chatModels } from '$lib/server/openrouter';
import { notSignedIn } from '$lib/server/proxy';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** The models the plan offers for chats, in OpenRouter's shape, with their prices. */
export const GET: RequestHandler = async ({ locals }) => {
	if (!locals.user) return notSignedIn();
	const { openrouter } = await getService();
	return json({ data: chatModels(await openrouter.models()) });
};
