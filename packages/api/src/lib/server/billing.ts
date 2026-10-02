import { eq } from 'drizzle-orm';
import type Stripe from 'stripe';
import { readAccount, updateAccount } from './accounts.ts';
import type { Db } from './db.ts';
import {
	addExtra,
	endPlan,
	grantPeriod,
	hasPlan,
	noPlan,
	startPlan,
	takeBack,
	type PeriodGrant
} from './limits.ts';
import { customer, user } from './schema.ts';

/*
 * Selling the nolune plan through Stripe (DESIGN.md, Payments). The products say what they grant
 * in their metadata, in cents. Checkout sells the plan (a subscription) and packs of extra credits
 * (one payment); the customer portal changes the card or cancels; and Stripe's events grant what
 * was paid for: a period's credits on each paid invoice, a pack's once its Checkout is paid, and
 * the plan's end once its subscription runs out. Every grant is keyed by what paid for it (the
 * invoice, the Checkout Session), so an event Stripe sends twice grants once.
 */

const DAY = 24 * 60 * 60 * 1000;
const HOUR = 60 * 60 * 1000;
/** The metadata counts cents; the ledger, millionths of a dollar. */
const MICROS_PER_CENT = 10_000;
/** A subscription that still holds the plan, whether or not its last payment went through. */
const LIVE = new Set<string>(['active', 'trialing', 'past_due', 'unpaid']);

export interface PlanOffer {
	kind: 'plan';
	credits: number;
	/** The 5-hour and weekly limits, when the tier has them (`limit_5h_cents`, `limit_week_cents`). */
	window: number | null;
	week: number | null;
	carryOver: number;
	/** The launch offer: more credits than the price, for a while (`offer: launch`). */
	launch: boolean;
}

export interface PackOffer {
	kind: 'pack';
	credits: number;
	expiresInDays: number;
}

/** What a product grants, from its metadata; null when it isn't one of nolune's. */
export function offerOf(product: {
	metadata?: Record<string, string> | null;
}): PlanOffer | PackOffer | null {
	const metadata = product.metadata ?? {};
	const micros = (key: string) => {
		const cents = Number(metadata[key]);
		return metadata[key] !== undefined && Number.isFinite(cents) && cents >= 0
			? Math.round(cents * MICROS_PER_CENT)
			: null;
	};
	const credits = micros('credits_cents');
	if (credits === null) return null;
	if (metadata.kind === 'plan') {
		return {
			kind: 'plan',
			credits,
			window: micros('limit_5h_cents'),
			week: micros('limit_week_cents'),
			carryOver: micros('rollover_cap_cents') ?? 0,
			launch: metadata.offer === 'launch'
		};
	}
	if (metadata.kind === 'pack') {
		return { kind: 'pack', credits, expiresInDays: Number(metadata.expires_in_days) || 365 };
	}
	return null;
}

/** A price nolune sells, as the account page shows it. */
export interface Priced<O> {
	priceId: string;
	/** In the currency's smallest unit. */
	amount: number | null;
	currency: string;
	name: string;
	description: string | null;
	offer: O;
}

export interface Catalog {
	plan: Priced<PlanOffer>;
	pack: Priced<PackOffer>;
}

/** Someone's subscription as Stripe has it. */
export interface SubscriptionState {
	status: Stripe.Subscription.Status;
	/** When it renews, while it goes on. */
	renewsAt: number | null;
	/** When it ends, once it's been cancelled. */
	endsAt: number | null;
}

/** The Stripe calls billing makes: Stripe's own client, or a stand-in in tests. */
export type StripeApi = Pick<
	Stripe,
	'customers' | 'checkout' | 'billingPortal' | 'subscriptions' | 'prices' | 'invoicePayments'
>;

export interface BillingOptions {
	db: Db;
	stripe: StripeApi;
	/** Where the account pages are, which Checkout and the portal come back to. */
	origin: string;
	/** The lookup keys of the plan's price and the pack's. */
	prices?: { plan: string; pack: string };
	/** Stripe as merchant of record: switched on in the Dashboard first. */
	managedPayments?: boolean;
	/**
	 * The customer portal's configuration: nolune's own, since the account's default is the
	 * other products' too and may offer switching to them.
	 */
	portalConfiguration?: string;
	now?: () => number;
	log?: Pick<Console, 'warn'>;
}

/** Why a checkout can't start, in words for the account page. */
export class BillingError extends Error {}

export class Billing {
	private readonly db: Db;
	private readonly stripe: StripeApi;
	private readonly origin: string;
	private readonly prices: { plan: string; pack: string };
	private readonly managedPayments: boolean;
	private readonly portalConfiguration: string | undefined;
	private readonly now: () => number;
	private readonly log: Pick<Console, 'warn'>;
	private listed: { at: number; catalog: Promise<Catalog> } | null = null;

	constructor(options: BillingOptions) {
		this.db = options.db;
		this.stripe = options.stripe;
		this.origin = options.origin.replace(/\/+$/, '');
		this.prices = options.prices ?? { plan: 'nolune-plan-family', pack: 'nolune-pack-10' };
		this.managedPayments = options.managedPayments ?? false;
		this.portalConfiguration = options.portalConfiguration;
		this.now = options.now ?? Date.now;
		this.log = options.log ?? console;
	}

	/** The plan's price and the pack's, with what each grants, kept for an hour. */
	catalog(): Promise<Catalog> {
		const now = this.now();
		if (!this.listed || now - this.listed.at > HOUR) {
			const catalog = this.list();
			this.listed = { at: now, catalog };
			catalog.catch(() => (this.listed = null));
		}
		return this.listed.catalog;
	}

	private async list(): Promise<Catalog> {
		const { plan, pack } = this.prices;
		const { data } = await this.stripe.prices.list({
			lookup_keys: [plan, pack],
			active: true,
			expand: ['data.product']
		});
		const priced = <K extends 'plan' | 'pack'>(key: string, kind: K) => {
			const price = data.find((p) => p.lookup_key === key);
			const product = price?.product as Stripe.Product | undefined;
			const offer = product ? offerOf(product) : null;
			if (!price || !product || offer?.kind !== kind) {
				throw new Error(`Stripe has no ${kind} price with the lookup key ${key}`);
			}
			return {
				priceId: price.id,
				amount: price.unit_amount,
				currency: price.currency,
				name: product.name,
				description: product.description,
				offer: offer as K extends 'plan' ? PlanOffer : PackOffer
			};
		};
		return { plan: priced(plan, 'plan'), pack: priced(pack, 'pack') };
	}

	private async customerOf(userId: string): Promise<string | null> {
		const [found] = await this.db.select().from(customer).where(eq(customer.userId, userId));
		return found?.stripeCustomerId ?? null;
	}

	/** Someone's Stripe customer, made the first time they check out. */
	async customerFor(person: { id: string; email: string }): Promise<string> {
		const found = await this.customerOf(person.id);
		if (found) return found;
		const made = await this.stripe.customers.create(
			{ email: person.email, metadata: { userId: person.id } },
			// Two clicks at once make one customer.
			{ idempotencyKey: `nolune-customer-${person.id}` }
		);
		await this.db
			.insert(customer)
			.values({ userId: person.id, stripeCustomerId: made.id })
			.onConflictDoNothing();
		return (await this.customerOf(person.id))!;
	}

	/** Someone's subscription that still holds the plan, as Stripe has it; null when there's none. */
	async subscription(userId: string): Promise<SubscriptionState | null> {
		const stripeCustomer = await this.customerOf(userId);
		if (!stripeCustomer) return null;
		const { data } = await this.stripe.subscriptions.list({
			customer: stripeCustomer,
			status: 'all',
			limit: 10
		});
		const live = data.find((s) => LIVE.has(s.status));
		if (!live) return null;
		const periodEnd = live.items.data[0]?.current_period_end;
		const endsAt = live.cancel_at
			? live.cancel_at * 1000
			: live.cancel_at_period_end && periodEnd
				? periodEnd * 1000
				: null;
		return {
			status: live.status,
			renewsAt: endsAt === null && periodEnd ? periodEnd * 1000 : null,
			endsAt
		};
	}

	/**
	 * A Checkout page for the plan or a pack of extra credits; its address. One plan per account,
	 * and packs only on top of one: they're spent past its limits.
	 */
	async checkout(person: { id: string; email: string }, what: 'plan' | 'pack'): Promise<string> {
		if (what === 'plan' && (await this.subscription(person.id))) {
			throw new BillingError('This account has the plan already.');
		}
		if (what === 'pack' && !hasPlan(await readAccount(this.db, person.id))) {
			throw new BillingError('Extra credits go on top of the plan: subscribe first.');
		}
		const { priceId } = (await this.catalog())[what];
		const session = await this.stripe.checkout.sessions.create({
			mode: what === 'plan' ? 'subscription' : 'payment',
			customer: await this.customerFor(person),
			client_reference_id: person.id,
			line_items: [{ price: priceId, quantity: 1 }],
			metadata: { userId: person.id },
			...(what === 'plan' ? { subscription_data: { metadata: { userId: person.id } } } : {}),
			...(this.managedPayments ? { managed_payments: { enabled: true } } : {}),
			// Back on the account page, which waits for the event that grants it.
			success_url: `${this.origin}/?checkout=${what}&session={CHECKOUT_SESSION_ID}`,
			cancel_url: `${this.origin}/`
		});
		if (!session.url) throw new Error(`Stripe gave Checkout Session ${session.id} no address`);
		return session.url;
	}

	/** Stripe's customer portal, to change the card or cancel; null for someone who never paid. */
	async portal(userId: string): Promise<string | null> {
		const stripeCustomer = await this.customerOf(userId);
		if (!stripeCustomer) return null;
		const session = await this.stripe.billingPortal.sessions.create({
			customer: stripeCustomer,
			return_url: `${this.origin}/`,
			...(this.portalConfiguration ? { configuration: this.portalConfiguration } : {})
		});
		return session.url;
	}

	/** Whose a customer is: by what their first checkout kept, else what the object says. */
	private async userOf(
		stripeCustomer: string | { id: string } | null,
		hint: string | null | undefined
	): Promise<string | null> {
		const id = typeof stripeCustomer === 'string' ? stripeCustomer : stripeCustomer?.id;
		if (id) {
			const [found] = await this.db
				.select()
				.from(customer)
				.where(eq(customer.stripeCustomerId, id));
			if (found) return found.userId;
		}
		if (!hint) return null;
		const [known] = await this.db.select({ id: user.id }).from(user).where(eq(user.id, hint));
		return known?.id ?? null;
	}

	/** What a Stripe event changes, once it's been checked (`readEvent`). */
	async handle(event: Stripe.Event): Promise<void> {
		switch (event.type) {
			case 'invoice.paid':
				return this.invoicePaid(event.data.object);
			case 'checkout.session.completed':
			case 'checkout.session.async_payment_succeeded':
				return this.checkoutPaid(event.data.object);
			case 'customer.subscription.deleted':
				return this.subscriptionEnded(event.data.object);
			case 'charge.refunded':
				return this.chargeRefunded(event.data.object);
		}
	}

	/** A period paid for: its credits, what carries over from the last, and the tier's limits. */
	private async invoicePaid(invoice: Stripe.Invoice): Promise<void> {
		const ref = invoice.parent?.subscription_details?.subscription;
		const subscriptionId = typeof ref === 'string' ? ref : ref?.id;
		if (!subscriptionId || !invoice.id) return;
		// The account sells other things too, whose invoices come here as well: nolune's are those
		// whose subscription says whose it is (Checkout puts it there), or whose customer is kept.
		const hint = invoice.parent?.subscription_details?.metadata?.userId;
		if (!(await this.userOf(invoice.customer, hint))) return;
		// A new subscription's first period and each one after; a prorated change of tier is for
		// when there's more than one.
		if (
			invoice.billing_reason !== 'subscription_create' &&
			invoice.billing_reason !== 'subscription_cycle'
		) {
			this.log.warn(`[nolune api] ${invoice.id} (${invoice.billing_reason}) grants nothing`);
			return;
		}
		const subscription = await this.stripe.subscriptions.retrieve(subscriptionId, {
			expand: ['items.data.price.product']
		});
		// An invoice paid late, or sent again, for a subscription that has run out since grants
		// nothing: its end took the period's credits, and with them what would say it was granted.
		if (!LIVE.has(subscription.status)) return;
		const item = subscription.items.data[0];
		const offer = item ? offerOf(item.price.product as Stripe.Product) : null;
		if (offer?.kind !== 'plan') {
			this.log.warn(`[nolune api] ${subscriptionId} isn't for a plan nolune sells`);
			return;
		}
		const userId = await this.userOf(subscription.customer, subscription.metadata?.userId);
		if (!userId) {
			this.log.warn(
				`[nolune api] nobody is ${String(subscription.customer)}, paying ${invoice.id}`
			);
			return;
		}
		const now = this.now();
		const period: PeriodGrant = {
			source: invoice.id,
			credits: offer.credits,
			carryOver: offer.carryOver,
			limits: { window: offer.window, week: offer.week },
			renewsAt: item.current_period_end * 1000
		};
		await updateAccount(this.db, userId, (account) =>
			hasPlan(account)
				? grantPeriod(account, period, now)
				: startPlan(period, now, account ?? undefined)
		);
	}

	/** A pack paid for: its credits, for as long as it lasts. */
	private async checkoutPaid(session: Stripe.Checkout.Session): Promise<void> {
		if (session.mode !== 'payment' || session.payment_status !== 'paid') return;
		// Someone's here, or the payment is for something else the account sells.
		const userId = await this.userOf(
			session.customer,
			session.client_reference_id ?? session.metadata?.userId
		);
		if (!userId) return;
		const { data: items } = await this.stripe.checkout.sessions.listLineItems(session.id, {
			expand: ['data.price.product']
		});
		let credits = 0;
		let days = 0;
		for (const item of items) {
			const offer = item.price ? offerOf(item.price.product as Stripe.Product) : null;
			if (offer?.kind !== 'pack') continue;
			credits += offer.credits * (item.quantity ?? 1);
			days = Math.max(days, offer.expiresInDays);
		}
		if (!credits) return;
		const now = this.now();
		await updateAccount(this.db, userId, (account) =>
			addExtra(account ?? noPlan(now), {
				source: session.id,
				credits,
				expiresAt: now + days * DAY
			})
		);
	}

	/**
	 * A payment refunded in full (from the Dashboard, or by Stripe itself under Managed Payments)
	 * takes back what's left of what it paid for: a period's credits or a pack's. A refund of part
	 * of a payment takes nothing, and is logged for whoever made it to settle.
	 */
	private async chargeRefunded(charge: Stripe.Charge): Promise<void> {
		// Someone's here, or the payment was for something else the account sells.
		const userId = await this.userOf(charge.customer, charge.metadata?.userId);
		if (!userId) return;
		const ref = charge.payment_intent;
		const paymentIntent = typeof ref === 'string' ? ref : ref?.id;
		if (!paymentIntent) return;
		if (!charge.refunded) {
			this.log.warn(
				`[nolune api] ${charge.id} was refunded in part (${charge.amount_refunded} of ${charge.amount}); its credits stay`
			);
			return;
		}
		const source = await this.paidFor(paymentIntent);
		if (!source) {
			this.log.warn(`[nolune api] ${charge.id} was refunded, and paid for nothing nolune granted`);
			return;
		}
		await updateAccount(this.db, userId, (account) => (account ? takeBack(account, source) : null));
	}

	/** What a payment paid for, as its credits' source: a period's invoice, or a pack's Checkout. */
	private async paidFor(paymentIntent: string): Promise<string | null> {
		const { data: payments } = await this.stripe.invoicePayments.list({
			payment: { type: 'payment_intent', payment_intent: paymentIntent },
			limit: 1
		});
		const invoice = payments[0]?.invoice;
		if (invoice) return typeof invoice === 'string' ? invoice : (invoice.id ?? null);
		const { data: sessions } = await this.stripe.checkout.sessions.list({
			payment_intent: paymentIntent,
			limit: 1
		});
		return sessions[0]?.id ?? null;
	}

	/** A subscription that ran out ends the plan, unless another one holds it now. */
	private async subscriptionEnded(subscription: Stripe.Subscription): Promise<void> {
		const userId = await this.userOf(subscription.customer, subscription.metadata?.userId);
		if (!userId) return;
		const stripeCustomer =
			typeof subscription.customer === 'string' ? subscription.customer : subscription.customer.id;
		const { data } = await this.stripe.subscriptions.list({
			customer: stripeCustomer,
			status: 'all',
			limit: 10
		});
		if (data.some((s) => s.id !== subscription.id && LIVE.has(s.status))) return;
		await updateAccount(this.db, userId, (account) => (account ? endPlan(account) : null));
	}
}

/** A webhook's event, once its signature says it's Stripe's; throws when it doesn't. */
export function readEvent(
	stripe: Pick<Stripe, 'webhooks'>,
	body: string,
	signature: string | null,
	secret: string
): Stripe.Event {
	return stripe.webhooks.constructEvent(body, signature ?? '', secret);
}
