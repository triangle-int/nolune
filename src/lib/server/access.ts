import { error } from '@sveltejs/kit';
import { getConversationForUser, getProfileForUser } from '@nolune/core';
import { translations } from '$lib/i18n';
import type { AuthUser } from './auth';

/** What went wrong, in the interface's language. */
function errors(locals: App.Locals) {
	return translations(locals.locale).m.errors;
}

export function requireUser(locals: App.Locals): AuthUser {
	if (!locals.user) error(401, errors(locals).notSignedIn);
	return locals.user;
}

export function requireAdmin(locals: App.Locals): AuthUser {
	const user = requireUser(locals);
	if (!user.isAdmin) error(403, errors(locals).adminsOnly);
	return user;
}

/** The profile, or 404 if it doesn't exist or the user isn't a member. */
export function requireProfile(locals: App.Locals, slug: string) {
	const user = requireUser(locals);
	const profile = getProfileForUser(slug, user.id);
	if (!profile) error(404, errors(locals).profileNotFound);
	return { user, profile };
}

export function requireConversation(locals: App.Locals, id: string) {
	const user = requireUser(locals);
	const found = getConversationForUser(id, user.id);
	if (!found) error(404, errors(locals).conversationNotFound);
	return { user, ...found };
}
