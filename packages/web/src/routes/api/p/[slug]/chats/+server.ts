import { error, json } from '@sveltejs/kit';
import { getDefaultPreset, listConversations, runningConversationIds } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { DEFAULT_CHATS, MAX_CHATS, chatSummary, decodeCursor, encodeCursor } from '$lib/server/api';
import { startChat } from '$lib/server/chats';
import type { RequestHandler } from './$types';

/**
 * A page of the profile's chats, most recently active first, as the sidebar lists them. `limit`
 * (up to 100) says how many; `next`, when there may be more, is the `after` for the page after.
 */
export const GET: RequestHandler = ({ params, locals, url }) => {
	const { profile } = requireProfile(locals, params.slug);
	const limit = Number(url.searchParams.get('limit') ?? DEFAULT_CHATS);
	if (!Number.isInteger(limit) || limit < 1 || limit > MAX_CHATS) {
		error(400, `limit is a whole number from 1 to ${MAX_CHATS}`);
	}
	const afterParam = url.searchParams.get('after');
	const after = afterParam === null ? undefined : decodeCursor(afterParam);
	if (after === null) error(400, "after is a page's next, as it was given");
	const page = listConversations(profile.id, { limit, after });
	const running = new Set(runningConversationIds());
	return json({
		chats: page.map((chat) => chatSummary(chat, running)),
		next: page.length === limit ? encodeCursor(page[page.length - 1]) : null
	});
};

/**
 * Starts a chat, as the new chat's page does: with the default model unless `preset` names one,
 * in `folder` if given, with `text` and `uploads` (ids from /api/p/<slug>/uploads) as its first
 * message when there are any.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const parsed: unknown = await request.json().catch(() => null);
	const body =
		parsed && typeof parsed === 'object' && !Array.isArray(parsed)
			? (parsed as Record<string, unknown>)
			: null;
	const text = body?.text ?? '';
	const uploads = body?.uploads ?? [];
	const folder = body?.folder ?? null;
	if (
		!body ||
		typeof text !== 'string' ||
		!Array.isArray(uploads) ||
		!uploads.every((id) => typeof id === 'string') ||
		(folder !== null && typeof folder !== 'string') ||
		[body.preset, body.effort, body.commands].some((v) => v !== undefined && typeof v !== 'string')
	) {
		error(400, 'Send JSON: text, uploads, folder, preset, effort and commands, each optional');
	}
	const started = await startChat(
		user,
		profile,
		{
			presetId: (body.preset as string | undefined) ?? getDefaultPreset()?.id ?? '',
			effort: (body.effort as string | undefined) ?? 'medium',
			text: text.trim(),
			uploads,
			folderId: folder || null,
			commands: body.commands as string | undefined
		},
		translations(locals.locale).m
	);
	if (!started.conversation) error(started.status, started.message);
	return json(chatSummary(started.conversation, new Set(runningConversationIds())), {
		status: 201
	});
};
