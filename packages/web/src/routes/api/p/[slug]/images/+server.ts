import { error, json } from '@sveltejs/kit';
import { getConversation, runningConversationIds } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { chatSummary } from '$lib/server/api';
import { imagesOverview, startPictureChat } from '$lib/server/images';
import type { RequestHandler } from './$types';

/** The profile's picture templates, and whether nolune can make pictures, as the Images page loads. */
export const GET: RequestHandler = ({ params, locals }) => {
	const { profile } = requireProfile(locals, params.slug);
	return json(imagesOverview(profile.slug, translations(locals.locale).m));
};

/**
 * Starts a chat that makes a picture, as the Images page does: `{ template, settings, uploads,
 * shape, extra }` for a template, or `{ text }` for a description. Answers with the new chat.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as Record<string, unknown> | null;
	if (!body) error(400, 'Send JSON');
	const text = (value: unknown) => (typeof value === 'string' ? value : undefined);
	const settings: Record<string, string> = {};
	if (body.settings && typeof body.settings === 'object') {
		for (const [key, value] of Object.entries(body.settings)) {
			if (typeof value === 'string') settings[key] = value;
		}
	}
	const uploads = Array.isArray(body.uploads)
		? body.uploads.filter((id) => typeof id === 'string')
		: [];
	const started = await startPictureChat(
		user,
		profile,
		{
			template: text(body.template) ?? '',
			uploads,
			settings,
			shape: text(body.shape),
			extra: text(body.extra),
			text: text(body.text)
		},
		translations(locals.locale).m
	);
	if ('problem' in started) error(400, started.problem);
	const chat = getConversation(started.conversationId);
	if (!chat) error(500, 'The chat went missing');
	return json(chatSummary(chat, new Set(runningConversationIds())), { status: 201 });
};
