import { fail, redirect } from '@sveltejs/kit';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { imagesOverview, startPictureChat } from '$lib/server/images';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	return imagesOverview(profile.slug, translations(locals.locale).m);
};

export const actions: Actions = {
	/**
	 * Starts a new chat that asks nolune for a picture: with the prompt a template builds from its
	 * settings, or from a description. Pictures were uploaded already (like files in the chat
	 * composer) and are attached to the message by their upload ids.
	 */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const template = form.get('template')?.toString() ?? '';
		const settings: Record<string, string> = {};
		for (const [key, value] of form.entries()) {
			if (key.startsWith('setting:')) settings[key.slice('setting:'.length)] = value.toString();
		}
		const started = await startPictureChat(
			user,
			profile,
			{
				template,
				uploads: form.getAll('upload').map(String),
				settings,
				shape: form.get('shape')?.toString(),
				extra: form.get('extra')?.toString(),
				text: form.get('text')?.toString()
			},
			translations(locals.locale).m
		);
		if ('problem' in started) return fail(400, { template, message: started.problem });
		redirect(303, `/p/${profile.slug}/c/${started.conversationId}`);
	}
};
