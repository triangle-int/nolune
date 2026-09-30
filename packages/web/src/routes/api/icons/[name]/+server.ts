import { error, json } from '@sveltejs/kit';
import { lucideIcon } from '$lib/server/icons';
import type { RequestHandler } from './$types';

/**
 * The drawing of one Lucide icon, by its kebab-case name (`cloud-sun`), for the icons the model
 * picks for each command. Served one at a time so pages don't download all two thousand.
 */
export const GET: RequestHandler = ({ params }) => {
	const node = lucideIcon(params.name);
	if (!node) error(404, 'Unknown icon');
	return json(node, { headers: { 'cache-control': 'private, max-age=86400' } });
};
