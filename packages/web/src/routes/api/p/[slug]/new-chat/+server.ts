import { json } from '@sveltejs/kit';
import {
	EFFORTS,
	commandMode,
	currentSuggestions,
	getDefaultPreset,
	listPresets
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { withIcons } from '$lib/server/suggestions';
import type { RequestHandler } from './$types';

/**
 * What a new chat starts with, for apps of their own, as the new-chat page loads it: the person's
 * chips (`suggestionsStale`: ask `/suggestions` for new ones), the models a chat can use and the
 * default one, the reasoning levels, and how commands run unless a chat says otherwise. A chat's
 * composer reads the models and levels here too.
 */
export const GET: RequestHandler = ({ locals, params }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const suggestions = currentSuggestions(profile.slug, { id: user.id, name: user.name });
	return json({
		suggestions: withIcons(suggestions.suggestions, translations(locals.locale).m),
		suggestionsStale: suggestions.stale,
		presets: listPresets().map((p) => ({ id: p.id, name: p.name, provider: p.provider })),
		defaultPresetId: getDefaultPreset()?.id ?? '',
		efforts: [...EFFORTS],
		commandMode: commandMode()
	});
};
