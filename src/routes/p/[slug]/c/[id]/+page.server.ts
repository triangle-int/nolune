import { error, fail, redirect } from '@sveltejs/kit';
import {
	EFFORTS,
	TitleError,
	deleteConversation,
	getConversation,
	renameConversation,
	stopConversation,
	subagentByConversation
} from '@btw/core';
import { requireConversation } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { conversation, profile } = requireConversation(locals, params.id);
	if (profile.slug !== params.slug) error(404, 'Conversation not found');
	const subagent = subagentByConversation(conversation.id);
	return {
		conversation: {
			id: conversation.id,
			title: conversation.title,
			presetName: conversation.presetName,
			effort: conversation.effort,
			contextWindow: conversation.contextWindow,
			hidden: conversation.hidden,
			cacheTtl: conversation.cacheTtl,
			/** A subagent's own chat: who started it, where. */
			subagent: subagent
				? {
						name: subagent.name,
						parentId: subagent.parentId,
						parentTitle: getConversation(subagent.parentId)?.title || 'a chat'
					}
				: null
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
		const { profile, user } = requireConversation(locals, params.id);
		// Nothing keeps working, or reports back, for a chat that's gone.
		stopConversation(params.id, user.name);
		deleteConversation(params.id);
		redirect(303, `/p/${profile.slug}`);
	}
};
