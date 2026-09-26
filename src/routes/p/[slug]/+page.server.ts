import { fail, redirect } from '@sveltejs/kit';
import {
	EFFORTS,
	createConversation,
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
	/** Starts a conversation, with its first message when one was typed. */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const presetId = form.get('preset')?.toString() ?? '';
		const effort = (form.get('effort')?.toString() ?? 'medium') as Effort;
		const text = form.get('text')?.toString().trim() ?? '';
		if (!getPreset(presetId)) return fail(400, { message: 'Pick a model.' });
		if (!EFFORTS.includes(effort)) return fail(400, { message: 'Pick a reasoning level.' });
		const conversation = createConversation({ profile, presetId, userId: user.id, effort });
		if (text) sendMessage(conversation.id, { id: user.id, name: user.name }, text);
		redirect(303, `/p/${profile.slug}/c/${conversation.id}`);
	}
};
