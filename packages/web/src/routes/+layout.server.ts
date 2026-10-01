import { availableUpdate, listNotificationsForUser } from '@nolune/core';
import { pictureUrl } from '$lib/pictures';
import { PREFERENCES_COOKIE, parsePreferences } from '$lib/preferences.svelte';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, depends, cookies }) => {
	depends('nolune:notifications');
	// Pushed when a member's name or picture changes, this person's own from another device too.
	depends('nolune:profiles');
	// Pushed to admins when the gateway hears of a new release.
	depends('nolune:update');
	const prefs = parsePreferences(cookies.get(PREFERENCES_COOKIE));
	const locale = locals.locale;
	if (!locals.user) return { user: null, notifications: null, update: null, prefs, locale };
	const isAdmin = locals.user.isAdmin === true;
	return {
		user: {
			id: locals.user.id,
			name: locals.user.name,
			email: locals.user.email,
			isAdmin,
			picture: pictureUrl(locals.user.picture)
		},
		notifications: listNotificationsForUser(locals.user.id),
		// A newer nolune, and how to update this one: admins look after it.
		update: isAdmin ? availableUpdate() : null,
		prefs,
		locale
	};
};
