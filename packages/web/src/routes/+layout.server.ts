import { listNotificationsForUser } from '@nolune/core';
import { pictureUrl } from '$lib/pictures';
import { PREFERENCES_COOKIE, parsePreferences } from '$lib/preferences.svelte';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, depends, cookies }) => {
	depends('nolune:notifications');
	// Pushed when a member's name or picture changes, this person's own from another device too.
	depends('nolune:profiles');
	const prefs = parsePreferences(cookies.get(PREFERENCES_COOKIE));
	const locale = locals.locale;
	if (!locals.user) return { user: null, notifications: null, prefs, locale };
	return {
		user: {
			id: locals.user.id,
			name: locals.user.name,
			email: locals.user.email,
			isAdmin: locals.user.isAdmin === true,
			picture: pictureUrl(locals.user.picture)
		},
		notifications: listNotificationsForUser(locals.user.id),
		prefs,
		locale
	};
};
