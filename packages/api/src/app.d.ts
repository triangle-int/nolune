import type { AuthSession, AuthUser } from '$lib/server/auth';

// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
	namespace App {
		interface Locals {
			/** Who's signed in: by the cookie on the account pages, by a bearer token on /v1. */
			user?: AuthUser;
			session?: AuthSession;
		}
	}
}

export {};
