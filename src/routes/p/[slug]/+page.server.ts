import { fail, redirect } from '@sveltejs/kit';
import { EFFORTS, createConversation, getPreset, listPresets, type Effort } from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	requireProfile(locals, params.slug);
	return {
		presets: listPresets().map((p) => ({ id: p.id, name: p.name })),
		efforts: [...EFFORTS]
	};
};

export const actions: Actions = {
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const presetId = form.get('preset')?.toString() ?? '';
		const effort = (form.get('effort')?.toString() ?? 'medium') as Effort;
		if (!getPreset(presetId)) return fail(400, { message: 'Pick a model.' });
		if (!EFFORTS.includes(effort)) return fail(400, { message: 'Pick a reasoning level.' });
		const conversation = createConversation({ profile, presetId, userId: user.id, effort });
		redirect(303, `/p/${profile.slug}/c/${conversation.id}`);
	}
};
