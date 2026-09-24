import { error, redirect } from '@sveltejs/kit';
import { EFFORTS, deleteConversation } from '@btw/core';
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
			contextWindow: conversation.contextWindow
		},
		efforts: [...EFFORTS]
	};
};

export const actions: Actions = {
	delete: async ({ locals, params }) => {
		const { profile } = requireConversation(locals, params.id);
		deleteConversation(params.id);
		redirect(303, `/p/${profile.slug}`);
	}
};
