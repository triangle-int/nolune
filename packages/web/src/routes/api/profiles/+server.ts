import { json } from '@sveltejs/kit';
import { listMembers, listProfilesForUser } from '@nolune/core';
import { pictureUrl } from '$lib/pictures';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** The profiles the person is a member of, with who else is in each. */
export const GET: RequestHandler = ({ locals }) => {
	const user = requireUser(locals);
	return json({
		profiles: listProfilesForUser(user.id).map((p) => ({
			slug: p.slug,
			name: p.name,
			avatar: p.avatar,
			members: listMembers(p.id).map((m) => ({
				id: m.id,
				name: m.name,
				picture: pictureUrl(m.picture)
			}))
		}))
	});
};
