import { fail, redirect } from '@sveltejs/kit';
import {
	EFFORTS,
	commandMode,
	currentSuggestions,
	getDefaultPreset,
	getFolder,
	listPresets
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { startChat } from '$lib/server/chats';
import { withIcons } from '$lib/server/suggestions';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params, url }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	// `?folder=<id>` starts the chat in that folder.
	const folderId = url.searchParams.get('folder');
	// Made for this person from the profile's memory. When it changed since, the page fetches new ones.
	const suggestions = currentSuggestions(profile.slug, { id: user.id, name: user.name });
	return {
		suggestions: withIcons(suggestions.suggestions, translations(locals.locale).m),
		suggestionsStale: suggestions.stale,
		presets: listPresets().map((p) => ({ id: p.id, name: p.name, provider: p.provider })),
		defaultPresetId: getDefaultPreset()?.id ?? '',
		efforts: [...EFFORTS],
		commandMode: commandMode(),
		folderId: folderId && getFolder(profile.id, folderId) ? folderId : null,
		// Picks the greeting, so the page the server renders and the browser's show the same one.
		greetingSeed: Math.random()
	};
};

export const actions: Actions = {
	/**
	 * Starts a conversation, in a folder if one was picked, with its first message when one was
	 * typed or files attached. The folder's page posts here too.
	 */
	default: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const started = await startChat(
			user,
			profile,
			{
				presetId: form.get('preset')?.toString() ?? '',
				effort: form.get('effort')?.toString() ?? 'medium',
				text: form.get('text')?.toString().trim() ?? '',
				uploads: form.getAll('upload').map(String),
				folderId: form.get('folder')?.toString() || null,
				commands: form.get('commands')?.toString()
			},
			translations(locals.locale).m
		);
		if (!started.conversation) return fail(started.status, { message: started.message });
		redirect(303, `/p/${profile.slug}/c/${started.conversation.id}`);
	}
};
