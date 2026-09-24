import { env } from '$env/dynamic/private';
import { getRequestEvent } from '$app/server';
import { betterAuth } from 'better-auth/minimal';
import { drizzleAdapter } from 'better-auth/adapters/drizzle';
import { sveltekitCookies } from 'better-auth/svelte-kit';
import { MIN_PASSWORD_LENGTH, getDb, readConfig, schema } from '@btw/core';

function createAuth() {
	return betterAuth({
		baseURL: env.ORIGIN || undefined,
		secret: readConfig().authSecret,
		database: drizzleAdapter(getDb(), { provider: 'sqlite', schema }),
		emailAndPassword: {
			enabled: true,
			// Accounts are created only with `btw user create`.
			disableSignUp: true,
			minPasswordLength: MIN_PASSWORD_LENGTH
		},
		user: {
			additionalFields: {
				isAdmin: { type: 'boolean', required: false, defaultValue: false, input: false }
			}
		},
		rateLimit: {
			enabled: true,
			window: 60,
			max: 100,
			customRules: { '/sign-in/email': { window: 60, max: 5 } }
		},
		plugins: [
			sveltekitCookies(getRequestEvent) // make sure this is the last plugin in the array
		]
	});
}

type Auth = ReturnType<typeof createAuth>;
let instance: Auth | undefined;

/** Created on first use, so importing this module (e.g. during `vite build`) has no side effects. */
export function getAuth(): Auth {
	instance ??= createAuth();
	return instance;
}

export type AuthUser = Auth['$Infer']['Session']['user'];
export type AuthSession = Auth['$Infer']['Session']['session'];
