import { listConversations } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, params, depends }) => {
	depends('btw:conversations');
	const { profile } = requireProfile(locals, params.slug);
	return {
		profile: { slug: profile.slug, name: profile.name },
		conversations: listConversations(profile.id).map((c) => ({
			id: c.id,
			title: c.title || 'New conversation',
			presetName: c.presetName,
			updatedAt: c.updatedAt.getTime()
		}))
	};
};
