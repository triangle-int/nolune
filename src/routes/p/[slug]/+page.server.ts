import { fail, redirect } from '@sveltejs/kit';
import {
	AttachmentError,
	EFFORTS,
	createConversation,
	findUploads,
	getDefaultPreset,
	getFolder,
	getPreset,
	listPresets,
	sendMessage,
	type Effort
} from '@btw/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params, url }) => {
	const { profile } = requireProfile(locals, params.slug);
	// `?folder=<id>` starts the chat in that folder.
	const folderId = url.searchParams.get('folder');
	return {
		presets: listPresets().map((p) => ({ id: p.id, name: p.name })),
		defaultPresetId: getDefaultPreset()?.id ?? '',
		efforts: [...EFFORTS],
		folderId: folderId && getFolder(profile.id, folderId) ? folderId : null
	};
};

export const actions: Actions = {
	/**
	 * Starts a conversation, in a folder if one was picked, with its first message when one was
	 * typed or files attached. The folder's page posts here too.
	 */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const presetId = form.get('preset')?.toString() ?? '';
		const effort = (form.get('effort')?.toString() ?? 'medium') as Effort;
		const text = form.get('text')?.toString().trim() ?? '';
		const uploads = form.getAll('upload').map(String);
		const folderId = form.get('folder')?.toString() || null;
		if (!getPreset(presetId)) return fail(400, { message: m.newChat.pickModel });
		if (!EFFORTS.includes(effort)) return fail(400, { message: m.newChat.pickEffort });
		if (folderId && !getFolder(profile.id, folderId)) {
			return fail(400, { message: m.newChat.folderGone });
		}
		try {
			// Checked before the conversation exists, so a stale file doesn't leave an empty chat.
			findUploads(profile.id, user.id, uploads);
		} catch (err) {
			if (err instanceof AttachmentError) return fail(400, { message: err.message });
			throw err;
		}
		const conversation = createConversation({
			profile,
			presetId,
			userId: user.id,
			effort,
			folderId
		});
		if (text || uploads.length) {
			await sendMessage(conversation.id, { id: user.id, name: user.name }, text, uploads);
		}
		redirect(303, `/p/${profile.slug}/c/${conversation.id}`);
	}
};
