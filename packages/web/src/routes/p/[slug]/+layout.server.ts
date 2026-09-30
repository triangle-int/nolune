import { listConversations, listFolders, listProfilesForUser } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { LAST_PROFILE_COOKIE } from '$lib/server/last-profile';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, params, depends, cookies }) => {
	depends('nolune:conversations');
	// Pushed when someone renames the profile or changes its avatar.
	depends('nolune:profiles');
	const { user, profile } = requireProfile(locals, params.slug);
	const { m } = translations(locals.locale);
	// Opening the app later lands here again.
	cookies.set(LAST_PROFILE_COOKIE, profile.slug, {
		path: '/',
		maxAge: 60 * 60 * 24 * 365,
		httpOnly: true,
		sameSite: 'lax'
	});
	return {
		profile: { slug: profile.slug, name: profile.name, avatar: profile.avatar },
		profiles: listProfilesForUser(user.id).map((p) => ({
			slug: p.slug,
			name: p.name,
			avatar: p.avatar
		})),
		folders: listFolders(profile.id).map((f) => ({ id: f.id, name: f.name })),
		conversations: listConversations(profile.id).map((c) => ({
			id: c.id,
			title: c.title || m.common.newChat,
			presetName: c.presetName,
			folderId: c.folderId,
			updatedAt: c.updatedAt.getTime()
		})),
		sidebarOpen: cookies.get('sidebar_state') !== 'false'
	};
};
