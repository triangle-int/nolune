import { env } from '$env/dynamic/private';
import { getRequestEvent } from '$app/server';
import { sveltekitCookies } from 'better-auth/svelte-kit';
import { createAuth, type Auth } from './auth.ts';
import { connect, type Db } from './db.ts';
import { emailSender } from './email.ts';

/*
 * The service's database and accounts, made once from the environment:
 *
 *   DATABASE_URL        Postgres
 *   BETTER_AUTH_SECRET  signs sessions; 32 random bytes or more
 *   ORIGIN              where the service is (adapter-node's), http://localhost:5173 in dev
 *   RESEND_API_KEY      sends sign-in codes; without it they're printed
 *   EMAIL_FROM          who they're from
 */

export interface Service {
	db: Db;
	auth: Auth;
}

let service: Promise<Service> | undefined;

export function getService(): Promise<Service> {
	service ??= start().catch((err) => {
		service = undefined;
		throw err;
	});
	return service;
}

function required(name: string): string {
	const value = env[name];
	if (!value) throw new Error(`${name} isn't set`);
	return value;
}

async function start(): Promise<Service> {
	const db = await connect(required('DATABASE_URL'));
	const auth = createAuth({
		db,
		secret: required('BETTER_AUTH_SECRET'),
		baseURL: env.ORIGIN || 'http://localhost:5173',
		sendEmail: emailSender({
			apiKey: env.RESEND_API_KEY,
			from: env.EMAIL_FROM || 'nolune <account@nolune.dev>'
		}),
		plugins: [sveltekitCookies(getRequestEvent)]
	});
	return { db, auth };
}
