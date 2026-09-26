import { fail, redirect } from '@sveltejs/kit';
import {
	AttachmentError,
	EFFORTS,
	createConversation,
	findUploads,
	getDefaultPreset,
	getPreset,
	listPresets,
	sendMessage,
	type Effort
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	requireProfile(locals, params.slug);
	return {
		presets: listPresets().map((p) => ({ id: p.id, name: p.name })),
		defaultPresetId: getDefaultPreset()?.id ?? '',
		efforts: [...EFFORTS]
	};
};

export const actions: Actions = {
	/** Starts a conversation, with its first message when one was typed or files attached. */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const presetId = form.get('preset')?.toString() ?? '';
		const effort = (form.get('effort')?.toString() ?? 'medium') as Effort;
		const text = form.get('text')?.toString().trim() ?? '';
		const uploads = form.getAll('upload').map(String);
		if (!getPreset(presetId)) return fail(400, { message: 'Pick a model.' });
		if (!EFFORTS.includes(effort)) return fail(400, { message: 'Pick a reasoning level.' });
		try {
			// Checked before the conversation exists, so a stale file doesn't leave an empty chat.
			findUploads(profile.id, user.id, uploads);
		} catch (err) {
			if (err instanceof AttachmentError) return fail(400, { message: err.message });
			throw err;
		}
		const conversation = createConversation({ profile, presetId, userId: user.id, effort });
		if (text || uploads.length) {
			await sendMessage(conversation.id, { id: user.id, name: user.name }, text, uploads);
		}
		redirect(303, `/p/${profile.slug}/c/${conversation.id}`);
	}
};
