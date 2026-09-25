import { getContext, setContext } from 'svelte';

/** Per-device display settings, kept in a cookie so the server renders them too. */
export const PREFERENCES_COOKIE = 'btw-prefs';

export interface PreferenceValues {
	/** Show the commands btw runs, token usage and prompt-cache details. */
	technical: boolean;
	/** Open the list of steps under each reply without clicking. */
	expandSteps: boolean;
}

export function parsePreferences(raw: string | undefined): PreferenceValues {
	let value: Partial<PreferenceValues> = {};
	try {
		value = JSON.parse(raw ?? '{}') ?? {};
	} catch {
		// A broken cookie falls back to the defaults.
	}
	return { technical: value.technical === true, expandSteps: value.expandSteps === true };
}

export class Preferences {
	technical = $state(false);
	expandSteps = $state(false);

	constructor(initial: PreferenceValues) {
		this.technical = initial.technical;
		this.expandSteps = initial.expandSteps;
	}

	set(values: Partial<PreferenceValues>) {
		if (values.technical !== undefined) this.technical = values.technical;
		if (values.expandSteps !== undefined) this.expandSteps = values.expandSteps;
		const cookie = encodeURIComponent(
			JSON.stringify({ technical: this.technical, expandSteps: this.expandSteps })
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
