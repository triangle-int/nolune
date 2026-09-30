import { getContext, setContext } from 'svelte';
import { isLocale, type Locale } from '$lib/i18n/locales';

/** Per-device display settings, kept in a cookie so the server renders them too. */
export const PREFERENCES_COOKIE = 'nolune-prefs';

/** A language for the interface, or `auto` for the one the browser asks for. */
export type LanguagePreference = Locale | 'auto';

export interface PreferenceValues {
	/** Show the commands nolune runs, token usage and prompt-cache details. */
	technical: boolean;
	/** Open the list of steps under each reply without clicking. */
	expandSteps: boolean;
	/** Soft sounds where the screen moves by itself, like a new profile's welcome. */
	sounds: boolean;
	/** The interface's language. Chats aren't translated: nolune answers in the language people write. */
	language: LanguagePreference;
}

export function parsePreferences(raw: string | undefined): PreferenceValues {
	let value: Partial<PreferenceValues> = {};
	try {
		value = JSON.parse(raw ?? '{}') ?? {};
	} catch {
		// A broken cookie falls back to the defaults.
	}
	return {
		technical: value.technical === true,
		expandSteps: value.expandSteps === true,
		sounds: value.sounds !== false,
		language: isLocale(value.language) ? value.language : 'auto'
	};
}

export class Preferences {
	technical = $state(false);
	expandSteps = $state(false);
	sounds = $state(true);
	language = $state<LanguagePreference>('auto');

	constructor(initial: PreferenceValues) {
		this.technical = initial.technical;
		this.expandSteps = initial.expandSteps;
		this.sounds = initial.sounds;
		this.language = initial.language;
	}

	set(values: Partial<PreferenceValues>) {
		if (values.technical !== undefined) this.technical = values.technical;
		if (values.expandSteps !== undefined) this.expandSteps = values.expandSteps;
		if (values.sounds !== undefined) this.sounds = values.sounds;
		if (values.language !== undefined) this.language = values.language;
		const cookie = encodeURIComponent(
			JSON.stringify({
				technical: this.technical,
				expandSteps: this.expandSteps,
				sounds: this.sounds,
				language: this.language
			})
		);
		document.cookie = `${PREFERENCES_COOKIE}=${cookie}; path=/; max-age=31536000; samesite=lax`;
	}
}

const KEY = Symbol('preferences');

export function setPreferences(preferences: Preferences): Preferences {
	return setContext(KEY, preferences);
}

export function getPreferences(): Preferences {
	return getContext<Preferences>(KEY);
}
