import type { Locale } from '$lib/i18n/locales';
import type { AuthSession, AuthUser } from '$lib/server/auth';

// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		interface Locals {
			user?: AuthUser;
			session?: AuthSession;
			/** The interface's language, from Settings or the browser (hooks.server.ts). */
			locale: Locale;
		}

		// interface Error {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};
