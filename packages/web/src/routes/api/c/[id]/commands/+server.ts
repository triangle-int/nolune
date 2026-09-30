import { error, json } from '@sveltejs/kit';
import { changeCommandMode, chatCommandChoice, isCommandMode } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireConversation } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * How the chat's commands run from its next one on. The chat keeps only what differs from Models &
 * keys. Anyone in the profile can have its commands checked; only an admin can turn that off.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user } = requireConversation(locals, params.id);
	const { m } = translations(locals.locale);
	const body = (await request.json().catch(() => null)) as { mode?: unknown } | null;
	const mode = typeof body?.mode === 'string' ? body.mode : '';
	if (!isCommandMode(mode)) error(400, 'Unknown command mode');
	const choice = chatCommandChoice(mode);
	if (choice === 'unrestricted' && !user.isAdmin) error(403, m.commandMode.adminsOnly);
	const commands = changeCommandMode(params.id, choice);
	if (!commands) error(404, m.errors.conversationNotFound);
	return json(commands);
};
