import { env } from '$env/dynamic/private';
import { getRequestEvent } from '$app/server';
import { sveltekitCookies } from 'better-auth/svelte-kit';
import Stripe from 'stripe';
import { createAuth, type Auth } from './auth.ts';
import { Billing } from './billing.ts';
import { connect, type Db } from './db.ts';
import { emailSender, type SendEmail } from './email.ts';
import { OpenRouter } from './openrouter.ts';
import { Proxy } from './proxy.ts';

/*
 * The service's database and accounts, made once from the environment:
 *
 *   DATABASE_URL        Postgres
 *   BETTER_AUTH_SECRET  signs sessions; 32 random bytes or more
 *   ORIGIN              where the service is (adapter-node's), http://localhost:5173 in dev
 *   RESEND_API_KEY      sends sign-in codes; without it (only at an http:// address) they're printed
 *   EMAIL_FROM          who they're from
 *   OPENROUTER_API_KEY  nolune's key at OpenRouter, which every plan's requests go on
 *   OPENROUTER_BASE_URL another address for it (a stand-in, in tests by hand)
 *   STRIPE_SECRET_KEY   sells the plan; without it there's nothing to buy
 *   STRIPE_WEBHOOK_SECRET  checks that Stripe's events are Stripe's
 *   STRIPE_MANAGED_PAYMENTS  true once Managed Payments is on in the Dashboard
 *   STRIPE_PORTAL_CONFIGURATION  nolune's customer portal (bpc_…): cancelling, cards, invoices
 *   STRIPE_PLAN_PRICE, STRIPE_PACK_PRICE  other lookup keys for the plan's price and the pack's
 *   STRIPE_API_URL      another address for Stripe's API (stripe-mock, or a stand-in by hand)
 */

export interface Service {
	db: Db;
	auth: Auth;
	openrouter: OpenRouter;
	proxy: Proxy;
	sendEmail: SendEmail;
	/** With Stripe's keys: Checkout, the portal and Stripe's events. */
	billing: { billing: Billing; stripe: Stripe; webhookSecret: string } | null;
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
	const sendEmail = emailSender({
		// At a public address, codes printed to the log would reach nobody: it needs Resend.
		apiKey: env.ORIGIN?.startsWith('https://') ? required('RESEND_API_KEY') : env.RESEND_API_KEY,
		from: env.EMAIL_FROM || 'nolune <account@mail.nolune.dev>'
	});
	const auth = createAuth({
		db,
		secret: required('BETTER_AUTH_SECRET'),
		baseURL: env.ORIGIN || 'http://localhost:5173',
		sendEmail,
		plugins: [sveltekitCookies(getRequestEvent)]
	});
	const openrouter = new OpenRouter({
		apiKey: required('OPENROUTER_API_KEY'),
		baseURL: env.OPENROUTER_BASE_URL
	});
	return {
		db,
		auth,
		openrouter,
		proxy: new Proxy({ db, openrouter }),
		billing: billing(db),
		sendEmail
	};
}

function billing(db: Db): Service['billing'] {
	if (!env.STRIPE_SECRET_KEY) return null;
	const standIn = env.STRIPE_API_URL ? new URL(env.STRIPE_API_URL) : null;
	const stripe = new Stripe(
		env.STRIPE_SECRET_KEY,
		standIn
			? {
					host: standIn.hostname,
					port: standIn.port,
					protocol: standIn.protocol.replace(':', '') as 'http' | 'https'
				}
			: {}
	);
	return {
		stripe,
		webhookSecret: required('STRIPE_WEBHOOK_SECRET'),
		billing: new Billing({
			db,
			stripe,
			origin: env.ORIGIN || 'http://localhost:5173',
			prices: {
				plan: env.STRIPE_PLAN_PRICE || 'nolune-plan-family',
				pack: env.STRIPE_PACK_PRICE || 'nolune-pack-10'
			},
			managedPayments: env.STRIPE_MANAGED_PAYMENTS === 'true',
			portalConfiguration: env.STRIPE_PORTAL_CONFIGURATION || undefined
		})
	};
}
