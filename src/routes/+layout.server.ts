import { listNotificationsForUser } from '@btw/core';
import { PREFERENCES_COOKIE, parsePreferences } from '$lib/preferences.svelte';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, depends, cookies }) => {
	depends('btw:notifications');
	const prefs = parsePreferences(cookies.get(PREFERENCES_COOKIE));
	const locale = locals.locale;
	if (!locals.user) return { user: null, notifications: null, prefs, locale };
	return {
		user: {
			id: locals.user.id,
			name: locals.user.name,
			email: locals.user.email,
			isAdmin: locals.user.isAdmin === true
		},
		notifications: listNotificationsForUser(locals.user.id),
		prefs,
		locale
	};
};
