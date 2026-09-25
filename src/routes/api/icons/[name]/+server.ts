import { error, json } from '@sveltejs/kit';
import { icons } from 'lucide';
import type { RequestHandler } from './$types';

/**
 * The drawing of one Lucide icon, by its kebab-case name (`cloud-sun`), for the icons the model
 * picks for each command. Served one at a time so pages don't download all two thousand.
 */
export const GET: RequestHandler = ({ params }) => {
	if (!/^[a-z0-9-]{1,64}$/.test(params.name)) error(404, 'Unknown icon');
	const key = params.name
		.split('-')
		.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
		.join('') as keyof typeof icons;
	const node = Object.hasOwn(icons, key) ? icons[key] : undefined;
	if (!node) error(404, 'Unknown icon');
	return json(node, { headers: { 'cache-control': 'private, max-age=86400' } });
};
