import { error } from '@sveltejs/kit';
import { getConversationForUser, getProfileForUser } from '@btw/core';
import type { AuthUser } from './auth';

export function requireUser(locals: App.Locals): AuthUser {
	if (!locals.user) error(401, 'Not signed in');
	return locals.user;
}

export function requireAdmin(locals: App.Locals): AuthUser {
	const user = requireUser(locals);
	if (!user.isAdmin) error(403, 'Admins only');
	return user;
}

/** The profile, or 404 if it doesn't exist or the user isn't a member. */
export function requireProfile(locals: App.Locals, slug: string) {
	const user = requireUser(locals);
	const profile = getProfileForUser(slug, user.id);
	if (!profile) error(404, 'Profile not found');
	return { user, profile };
}

export function requireConversation(locals: App.Locals, id: string) {
	const user = requireUser(locals);
	const found = getConversationForUser(id, user.id);
	if (!found) error(404, 'Conversation not found');
	return { user, ...found };
}
