import { error, fail, redirect } from '@sveltejs/kit';
import { EFFORTS, TitleError, deleteConversation, renameConversation } from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { conversation, profile } = requireConversation(locals, params.id);
	if (profile.slug !== params.slug) error(404, 'Conversation not found');
	return {
		conversation: {
			id: conversation.id,
			title: conversation.title,
			presetName: conversation.presetName,
			effort: conversation.effort,
			contextWindow: conversation.contextWindow,
			hidden: conversation.hidden
		},
		efforts: [...EFFORTS]
	};
};

export const actions: Actions = {
	rename: async ({ locals, params, request }) => {
		requireConversation(locals, params.id);
		const form = await request.formData();
		try {
			renameConversation(params.id, form.get('title')?.toString() ?? '');
		} catch (err) {
			if (err instanceof TitleError) return fail(400, { message: err.message });
			throw err;
		}
	},

	delete: async ({ locals, params }) => {
		const { profile } = requireConversation(locals, params.id);
		deleteConversation(params.id);
		redirect(303, `/p/${profile.slug}`);
	}
};
