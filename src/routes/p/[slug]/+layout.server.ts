import { listConversations, listFolders, listProfilesForUser } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import { LAST_PROFILE_COOKIE } from '$lib/server/last-profile';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, params, depends, cookies }) => {
	depends('btw:conversations');
	const { user, profile } = requireProfile(locals, params.slug);
	// Opening the app later lands here again.
	cookies.set(LAST_PROFILE_COOKIE, profile.slug, {
		path: '/',
		maxAge: 60 * 60 * 24 * 365,
		httpOnly: true,
		sameSite: 'lax'
	});
	return {
		profile: { slug: profile.slug, name: profile.name },
		profiles: listProfilesForUser(user.id).map((p) => ({ slug: p.slug, name: p.name })),
		folders: listFolders(profile.id).map((f) => ({ id: f.id, name: f.name })),
		conversations: listConversations(profile.id).map((c) => ({
			id: c.id,
			title: c.title || 'New chat',
			presetName: c.presetName,
			folderId: c.folderId,
			updatedAt: c.updatedAt.getTime()
		})),
		sidebarOpen: cookies.get('sidebar_state') !== 'false'
	};
};
