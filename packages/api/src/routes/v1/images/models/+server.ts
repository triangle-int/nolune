import { json } from '@sveltejs/kit';
import { notSignedIn } from '$lib/server/proxy';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** The models the plan offers for pictures, as OpenRouter's Image API lists them. */
export const GET: RequestHandler = async ({ locals }) => {
	if (!locals.user) return notSignedIn();
	const { openrouter } = await getService();
	return json({ data: await openrouter.imageModels() });
};
