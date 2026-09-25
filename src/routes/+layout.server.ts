import { listNotificationsForUser } from '@btw/core';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals, depends }) => {
	depends('btw:notifications');
	if (!locals.user) return { user: null, notifications: null };
	return {
		user: { id: locals.user.id, name: locals.user.name, isAdmin: locals.user.isAdmin === true },
		notifications: listNotificationsForUser(locals.user.id)
	};
};
