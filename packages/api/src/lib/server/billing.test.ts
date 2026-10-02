import Stripe from 'stripe';
import { describe, expect, it } from 'vitest';
import { readAccount, updateAccount } from './accounts.ts';
import { Billing, BillingError, offerOf, readEvent, type StripeApi } from './billing.ts';
import { HOUR, startPlan } from './limits.ts';
import { addUser, testDb } from './test/db.ts';

const t0 = Date.UTC(2026, 9, 1, 18, 0);
const DAY = 24 * HOUR;
const seconds = (ms: number) => Math.floor(ms / 1000);

// The products as they are in Stripe (test mode), metadata and all.
const family = {
	id: 'prod_VMSevRdZtLv2Wg',
	object: 'product',
	name: 'nolune Family',
	description: 'Launch offer: $25 of model use every month for $20, for the whole family.',
	metadata: {
		credits_cents: '2500',
		kind: 'plan',
		limit_5h_cents: '180',
		limit_week_cents: '875',
		nolune_product_id: 'plan-family',
		offer: 'launch',
		rollover_cap_cents: '1250'
	}
};
const pack = {
	id: 'prod_VMSekakNlUD47b',
	object: 'product',
	name: 'nolune extra credits',
	description: "Launch offer: all $10 as model use, one-off, spent only past the plan's limits.",
	metadata: {
		credits_cents: '1000',
		expires_in_days: '365',
		kind: 'pack',
		nolune_product_id: 'pack-10',
		offer: 'launch'
	}
};
const planPrice = {
	id: 'price_1ULjjRPfXApM9aK94vspo6ZY',
	lookup_key: 'nolune-plan-family',
	unit_amount: 2000,
	currency: 'usd',
	product: family
};
const packPrice = {
	id: 'price_1ULjjVPfXApM9aK9oP9OopiU',
	lookup_key: 'nolune-pack-10',
	unit_amount: 1000,
	currency: 'usd',
	product: pack
};

interface FakeSubscription {
	id: string;
	customer: string;
	status: Stripe.Subscription.Status;
	periodEnd: number;
	cancelAtPeriodEnd?: boolean;
	cancelAt?: number;
	userId?: string;
}

/** Stripe as far as billing calls it: what it was asked, and the subscriptions it has. */
function fakeStripe() {
	const calls: { name: string; params: unknown; options?: unknown }[] = [];
	const subscriptions: FakeSubscription[] = [];
	const lineItems = new Map<string, { price: object; quantity: number }[]>();
	let customers = 0;
	const subscriptionObject = (s: FakeSubscription) => ({
		id: s.id,
		object: 'subscription',
		customer: s.customer,
		status: s.status,
		cancel_at: s.cancelAt ? seconds(s.cancelAt) : null,
		cancel_at_period_end: s.cancelAtPeriodEnd ?? false,
		metadata: s.userId ? { userId: s.userId } : {},
		items: { data: [{ price: planPrice, current_period_end: seconds(s.periodEnd) }] }
	});
	const api = {
		prices: {
			list: async (params: unknown) => {
				calls.push({ name: 'prices.list', params });
				return { data: [planPrice, packPrice] };
			}
		},
		customers: {
			create: async (params: unknown, options: unknown) => {
				calls.push({ name: 'customers.create', params, options });
				return { id: `cus_${++customers}` };
			}
		},
		checkout: {
			sessions: {
				create: async (params: { mode: string }) => {
					calls.push({ name: 'checkout.sessions.create', params });
					return { id: 'cs_test_1', url: 'https://checkout.stripe.com/c/pay/cs_test_1' };
				},
				listLineItems: async (id: string, params: unknown) => {
					calls.push({
						name: 'checkout.sessions.listLineItems',
						params: { id, ...(params as object) }
					});
					return { data: lineItems.get(id) ?? [] };
				}
			}
		},
		billingPortal: {
			sessions: {
				create: async (params: unknown) => {
					calls.push({ name: 'billingPortal.sessions.create', params });
					return { url: 'https://billing.stripe.com/p/session/test_1' };
				}
			}
		},
		subscriptions: {
			list: async (params: { customer: string }) => {
				calls.push({ name: 'subscriptions.list', params });
				return {
					data: subscriptions.filter((s) => s.customer === params.customer).map(subscriptionObject)
				};
			},
			retrieve: async (id: string, params: unknown) => {
				calls.push({ name: 'subscriptions.retrieve', params: { id, ...(params as object) } });
				const found = subscriptions.find((s) => s.id === id);
				if (!found) throw new Error(`No such subscription: ${id}`);
				return subscriptionObject(found);
			}
		}
	};
	return { api: api as unknown as StripeApi, calls, subscriptions, lineItems };
}

async function setUp(options: { managedPayments?: boolean; portalConfiguration?: string } = {}) {
	const db = await testDb();
	const id = await addUser(db);
	const stripe = fakeStripe();
	let now = t0;
	const warnings: string[] = [];
	const billing = new Billing({
		db,
		stripe: stripe.api,
		origin: 'https://nolune.dev/',
		managedPayments: options.managedPayments,
		portalConfiguration: options.portalConfiguration,
		now: () => now,
		log: { warn: (message: string) => warnings.push(message) }
	});
	const person = { id, email: 'u1@example.com' };
	return {
		db,
		billing,
		stripe,
		person,
		warnings,
		at: (time: number) => (now = time)
	};
}

/** An event as Stripe sends it, as far as billing reads it. */
function event(type: string, object: object): Stripe.Event {
	return { id: `evt_${type}`, object: 'event', type, data: { object } } as unknown as Stripe.Event;
}

/** As Stripe sends it: the subscription's metadata (the userId Checkout put there) comes along. */
const invoicePaid = (
	id: string,
	subscription: string,
	billingReason: Stripe.Invoice.BillingReason = 'subscription_create',
	{ customer = 'cus_1', userId }: { customer?: string; userId?: string } = {}
) =>
	event('invoice.paid', {
		id,
		object: 'invoice',
		billing_reason: billingReason,
		customer,
		parent: {
			type: 'subscription_details',
			subscription_details: { subscription, metadata: userId ? { userId } : {} }
		}
	});

const checkoutCompleted = (id: string, paid = true, type = 'checkout.session.completed') =>
	event(type, {
		id,
		object: 'checkout.session',
		mode: 'payment',
		payment_status: paid ? 'paid' : 'unpaid',
		customer: 'cus_1',
		client_reference_id: 'u1',
		metadata: { userId: 'u1' }
	});

describe('what a product grants', () => {
	it('reads the plan and the pack from their metadata, in micro-dollars', () => {
		expect(offerOf(family)).toEqual({
			kind: 'plan',
			credits: 25_000_000,
			window: 1_800_000,
			week: 8_750_000,
			carryOver: 12_500_000,
			launch: true
		});
		expect(offerOf(pack)).toEqual({ kind: 'pack', credits: 10_000_000, expiresInDays: 365 });
	});

	it("isn't one of nolune's without its amounts", () => {
		expect(offerOf({ metadata: {} })).toBeNull();
		expect(offerOf({ metadata: { kind: 'plan', credits_cents: '2500' } })).toBeNull();
		expect(offerOf({ metadata: { kind: 'gensprite', credits_cents: '2500' } })).toBeNull();
		expect(offerOf({ metadata: { kind: 'pack', credits_cents: 'lots' } })).toBeNull();
	});
});

describe('buying', () => {
	it('finds the prices by their lookup keys, once an hour', async () => {
		const { billing, stripe, at } = await setUp();
		const catalog = await billing.catalog();
		expect(catalog.plan).toMatchObject({
			priceId: planPrice.id,
			amount: 2000,
			currency: 'usd',
			name: 'nolune Family',
			offer: { kind: 'plan', launch: true }
		});
		expect(catalog.pack).toMatchObject({ priceId: packPrice.id, amount: 1000 });
		await billing.catalog();
		at(t0 + 2 * HOUR);
		await billing.catalog();
		const lists = stripe.calls.filter((c) => c.name === 'prices.list');
		expect(lists).toHaveLength(2);
		expect(lists[0].params).toEqual({
			lookup_keys: ['nolune-plan-family', 'nolune-pack-10'],
			active: true,
			expand: ['data.product']
		});
	});

	it('opens Checkout for the plan, with one Stripe customer per person', async () => {
		const { db, billing, stripe, person } = await setUp({ managedPayments: true });
		const url = await billing.checkout(person, 'plan');
		expect(url).toBe('https://checkout.stripe.com/c/pay/cs_test_1');
		const [created] = stripe.calls.filter((c) => c.name === 'customers.create');
		expect(created).toMatchObject({
			params: { email: 'u1@example.com', metadata: { userId: 'u1' } },
			options: { idempotencyKey: 'nolune-customer-u1' }
		});
		expect(stripe.calls.find((c) => c.name === 'checkout.sessions.create')?.params).toEqual({
			mode: 'subscription',
			customer: 'cus_1',
			client_reference_id: 'u1',
			line_items: [{ price: planPrice.id, quantity: 1 }],
			metadata: { userId: 'u1' },
			subscription_data: { metadata: { userId: 'u1' } },
			managed_payments: { enabled: true },
			success_url: 'https://nolune.dev/?checkout=plan&session={CHECKOUT_SESSION_ID}',
			cancel_url: 'https://nolune.dev/'
		});

		// Leaving Checkout and coming back is the same customer.
		await billing.checkout(person, 'plan');
		expect(stripe.calls.filter((c) => c.name === 'customers.create')).toHaveLength(1);
		expect(await billing.customerFor(person)).toBe('cus_1');
		expect(await readAccount(db, person.id)).toBeNull();
	});

	it('sells one plan per account, and packs only on top of one', async () => {
		const { db, billing, stripe, person } = await setUp();
		await expect(billing.checkout(person, 'pack')).rejects.toBeInstanceOf(BillingError);

		await billing.customerFor(person);
		stripe.subscriptions.push({
			id: 'sub_1',
			customer: 'cus_1',
			status: 'active',
			periodEnd: t0 + 30 * DAY
		});
		await expect(billing.checkout(person, 'plan')).rejects.toThrow('has the plan already');

		await billing.handle(invoicePaid('in_1', 'sub_1'));
		await billing.checkout(person, 'pack');
		const sessions = stripe.calls.filter((c) => c.name === 'checkout.sessions.create');
		expect(sessions.at(-1)?.params).toMatchObject({
			mode: 'payment',
			line_items: [{ price: packPrice.id, quantity: 1 }],
			success_url: 'https://nolune.dev/?checkout=pack&session={CHECKOUT_SESSION_ID}'
		});
		expect(sessions.at(-1)?.params).not.toHaveProperty('subscription_data');
		expect(sessions.at(-1)?.params).not.toHaveProperty('managed_payments');
		expect((await readAccount(db, person.id))?.limits.week).toBe(8_750_000);
	});

	it("says how a subscription stands, and opens Stripe's portal", async () => {
		const { billing, stripe, person } = await setUp({ portalConfiguration: 'bpc_nolune' });
		expect(await billing.subscription(person.id)).toBeNull();
		expect(await billing.portal(person.id)).toBeNull();

		await billing.customerFor(person);
		stripe.subscriptions.push(
			{ id: 'sub_0', customer: 'cus_1', status: 'canceled', periodEnd: t0 },
			{ id: 'sub_1', customer: 'cus_1', status: 'active', periodEnd: t0 + 30 * DAY }
		);
		expect(await billing.subscription(person.id)).toEqual({
			status: 'active',
			renewsAt: seconds(t0 + 30 * DAY) * 1000,
			endsAt: null
		});
		stripe.subscriptions[1].cancelAtPeriodEnd = true;
		expect(await billing.subscription(person.id)).toEqual({
			status: 'active',
			renewsAt: null,
			endsAt: seconds(t0 + 30 * DAY) * 1000
		});

		// Cancelled in the portal, on flexible billing: `cancel_at`, and not at the period's end.
		stripe.subscriptions[1].cancelAtPeriodEnd = false;
		stripe.subscriptions[1].cancelAt = t0 + 30 * DAY;
		expect(await billing.subscription(person.id)).toMatchObject({
			renewsAt: null,
			endsAt: seconds(t0 + 30 * DAY) * 1000
		});

		// Cancelled in the portal, on flexible billing: `cancel_at`, and not at the period's end.
		stripe.subscriptions[1].cancelAtPeriodEnd = false;
		stripe.subscriptions[1].cancelAt = t0 + 30 * DAY;
		expect(await billing.subscription(person.id)).toMatchObject({
			renewsAt: null,
			endsAt: seconds(t0 + 30 * DAY) * 1000
		});

		expect(await billing.portal(person.id)).toBe('https://billing.stripe.com/p/session/test_1');
		expect(stripe.calls.find((c) => c.name === 'billingPortal.sessions.create')?.params).toEqual({
			customer: 'cus_1',
			return_url: 'https://nolune.dev/',
			configuration: 'bpc_nolune'
		});
	});
});

describe("Stripe's events", () => {
	it('start the plan on its first invoice, and grant each one once', async () => {
		const { db, billing, stripe, person, at } = await setUp();
		await billing.customerFor(person);
		const periodEnd = t0 + 30 * DAY;
		stripe.subscriptions.push({ id: 'sub_1', customer: 'cus_1', status: 'active', periodEnd });

		await billing.handle(invoicePaid('in_1', 'sub_1'));
		const started = await readAccount(db, person.id);
		expect(started).toEqual({
			limits: { window: 1_800_000, week: 8_750_000 },
			startedAt: t0,
			renewsAt: seconds(periodEnd) * 1000,
			window: null,
			week: null,
			credits: [{ source: 'in_1', kind: 'plan', left: 25_000_000, expiresAt: null }],
			extraPastLimits: false
		});
		expect(stripe.calls.find((c) => c.name === 'subscriptions.retrieve')?.params).toEqual({
			id: 'sub_1',
			expand: ['items.data.price.product']
		});

		// Stripe sends it again.
		at(t0 + HOUR);
		await billing.handle(invoicePaid('in_1', 'sub_1'));
		expect(await readAccount(db, person.id)).toEqual(started);

		// The next month: its credits, and up to $12.50 of what's left carried over.
		at(periodEnd);
		stripe.subscriptions[0].periodEnd = periodEnd + 31 * DAY;
		await billing.handle(invoicePaid('in_2', 'sub_1', 'subscription_cycle'));
		expect(await readAccount(db, person.id)).toMatchObject({
			startedAt: t0,
			renewsAt: seconds(periodEnd + 31 * DAY) * 1000,
			credits: [
				{ source: 'in_2', kind: 'plan', left: 25_000_000 },
				{ source: 'in_2:carried', kind: 'plan', left: 12_500_000 }
			]
		});
	});

	it("grant nothing for an invoice that isn't a period's, or a subscription that ran out", async () => {
		const { db, billing, stripe, person, warnings } = await setUp();
		await billing.customerFor(person);
		stripe.subscriptions.push(
			{ id: 'sub_1', customer: 'cus_1', status: 'active', periodEnd: t0 + 30 * DAY },
			{ id: 'sub_0', customer: 'cus_1', status: 'canceled', periodEnd: t0 }
		);
		await billing.handle(invoicePaid('in_1', 'sub_1', 'subscription_update'));
		await billing.handle(invoicePaid('in_0', 'sub_0', 'subscription_cycle'));
		await billing.handle(
			event('invoice.paid', {
				id: 'in_9',
				object: 'invoice',
				billing_reason: 'manual',
				parent: null
			})
		);
		expect(await readAccount(db, person.id)).toBeNull();
		expect(warnings).toEqual(['[nolune api] in_1 (subscription_update) grants nothing']);
	});

	it('find someone by the subscription they checked out with, before any customer is kept', async () => {
		const { db, billing, stripe, person } = await setUp();
		stripe.subscriptions.push({
			id: 'sub_1',
			customer: 'cus_elsewhere',
			status: 'active',
			periodEnd: t0 + 30 * DAY,
			userId: person.id
		});
		await billing.handle(
			invoicePaid('in_1', 'sub_1', 'subscription_create', {
				customer: 'cus_elsewhere',
				userId: person.id
			})
		);
		expect((await readAccount(db, person.id))?.limits.week).toBe(8_750_000);
	});

	it('pass over what the account sells besides nolune, without asking Stripe or warning', async () => {
		const { db, billing, stripe, person, warnings } = await setUp();
		// Another product's subscription and payment: a customer nobody here is, no userId.
		await billing.handle(
			invoicePaid('in_other', 'sub_other', 'subscription_cycle', { customer: 'cus_other' })
		);
		await billing.handle(
			event('checkout.session.completed', {
				id: 'cs_other',
				mode: 'payment',
				payment_status: 'paid',
				customer: 'cus_other',
				client_reference_id: 'someone-elses-user',
				metadata: {}
			})
		);
		await billing.handle(
			event('customer.subscription.deleted', {
				id: 'sub_other',
				object: 'subscription',
				customer: 'cus_other',
				status: 'canceled',
				metadata: {}
			})
		);
		expect(stripe.calls).toEqual([]);
		expect(warnings).toEqual([]);
		expect(await readAccount(db, person.id)).toBeNull();
	});

	it('add a pack once its Checkout is paid, and keep it when the plan ends', async () => {
		const { db, billing, stripe, person, at } = await setUp();
		await billing.customerFor(person);
		stripe.subscriptions.push({
			id: 'sub_1',
			customer: 'cus_1',
			status: 'active',
			periodEnd: t0 + 30 * DAY
		});
		await billing.handle(invoicePaid('in_1', 'sub_1'));
		stripe.lineItems.set('cs_2', [{ price: packPrice, quantity: 1 }]);

		// A bank transfer that hasn't come yet, then has.
		await billing.handle(checkoutCompleted('cs_2', false));
		expect((await readAccount(db, person.id))?.credits).toHaveLength(1);
		await billing.handle(
			checkoutCompleted('cs_2', true, 'checkout.session.async_payment_succeeded')
		);
		await billing.handle(checkoutCompleted('cs_2'));
		expect((await readAccount(db, person.id))?.credits).toEqual([
			{ source: 'in_1', kind: 'plan', left: 25_000_000, expiresAt: null },
			{ source: 'cs_2', kind: 'extra', left: 10_000_000, expiresAt: t0 + 365 * DAY }
		]);

		// Cancelled, and run out.
		stripe.subscriptions[0].status = 'canceled';
		await billing.handle(
			event('customer.subscription.deleted', {
				id: 'sub_1',
				object: 'subscription',
				customer: 'cus_1',
				status: 'canceled',
				metadata: {}
			})
		);
		expect(await readAccount(db, person.id)).toMatchObject({
			limits: { window: 0, week: 0 },
			renewsAt: null,
			credits: [{ source: 'cs_2', kind: 'extra', left: 10_000_000 }]
		});

		// Subscribing again starts a new plan, the pack still there.
		at(t0 + 40 * DAY);
		stripe.subscriptions.push({
			id: 'sub_2',
			customer: 'cus_1',
			status: 'active',
			periodEnd: t0 + 70 * DAY
		});
		await billing.handle(invoicePaid('in_3', 'sub_2'));
		expect(await readAccount(db, person.id)).toMatchObject({
			limits: { window: 1_800_000, week: 8_750_000 },
			startedAt: t0 + 40 * DAY,
			credits: [
				{ source: 'cs_2', kind: 'extra' },
				{ source: 'in_3', kind: 'plan', left: 25_000_000 }
			]
		});
	});

	it('keep the plan when a subscription ends while another holds it', async () => {
		const { db, billing, stripe, person } = await setUp();
		await billing.customerFor(person);
		stripe.subscriptions.push(
			{ id: 'sub_1', customer: 'cus_1', status: 'canceled', periodEnd: t0 },
			{ id: 'sub_2', customer: 'cus_1', status: 'active', periodEnd: t0 + 30 * DAY }
		);
		await updateAccount(db, person.id, () =>
			startPlan(
				{
					source: 'in_2',
					credits: 25_000_000,
					carryOver: 0,
					limits: { window: 1_800_000, week: 8_750_000 },
					renewsAt: t0 + 30 * DAY
				},
				t0
			)
		);
		await billing.handle(
			event('customer.subscription.deleted', {
				id: 'sub_1',
				object: 'subscription',
				customer: 'cus_1',
				status: 'canceled',
				metadata: {}
			})
		);
		expect((await readAccount(db, person.id))?.limits.week).toBe(8_750_000);
	});

	it('keep a pack paid for with no plan, for when there is one', async () => {
		const { db, billing, stripe, person } = await setUp();
		await billing.customerFor(person);
		stripe.lineItems.set('cs_1', [{ price: packPrice, quantity: 2 }]);
		await billing.handle(checkoutCompleted('cs_1'));
		expect(await readAccount(db, person.id)).toMatchObject({
			limits: { window: 0, week: 0 },
			credits: [{ source: 'cs_1', kind: 'extra', left: 20_000_000 }]
		});
	});

	it('pass over Checkouts for anything else', async () => {
		const { db, billing, stripe, person } = await setUp();
		await billing.customerFor(person);
		stripe.lineItems.set('cs_1', [
			{ price: { ...packPrice, product: { ...pack, metadata: {} } }, quantity: 1 }
		]);
		await billing.handle(checkoutCompleted('cs_1'));
		await billing.handle(
			event('checkout.session.completed', {
				id: 'cs_sub',
				mode: 'subscription',
				payment_status: 'paid',
				customer: 'cus_1'
			})
		);
		expect(await readAccount(db, person.id)).toBeNull();
		expect(stripe.calls.filter((c) => c.name === 'checkout.sessions.listLineItems')).toHaveLength(
			1
		);
	});

	it("are read only when they're signed with the endpoint's secret", () => {
		const stripe = new Stripe('sk_test_none');
		const body = JSON.stringify(invoicePaid('in_1', 'sub_1'));
		const secret = 'whsec_test';
		const signature = Stripe.webhooks.generateTestHeaderString({ payload: body, secret });
		expect(readEvent(stripe, body, signature, secret).type).toBe('invoice.paid');
		expect(() => readEvent(stripe, body, signature, 'whsec_other')).toThrow();
		expect(() => readEvent(stripe, body, null, secret)).toThrow();
		expect(() => readEvent(stripe, body.replace('in_1', 'in_2'), signature, secret)).toThrow();
	});
});
