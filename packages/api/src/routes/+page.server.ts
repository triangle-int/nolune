import { fail, redirect } from '@sveltejs/kit';
import { readAccount, updateAccount } from '$lib/server/accounts';
import { BillingError, type Catalog } from '$lib/server/billing';
import { hasPlan, usage } from '$lib/server/limits';
import { getService } from '$lib/server/service';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	if (!locals.user) redirect(303, '/sign-in');
	const { db, billing } = await getService();
	const account = await readAccount(db, locals.user.id);
	let catalog: Catalog | null = null;
	let subscription = null;
	let billingError: string | null = null;
	if (billing) {
		try {
			[catalog, subscription] = await Promise.all([
				billing.billing.catalog(),
				billing.billing.subscription(locals.user.id)
			]);
		} catch (err) {
			console.error('[nolune api] Stripe:', err);
			billingError = "Stripe can't be reached just now. Try again in a little while.";
		}
	}
	// Back from Checkout: whether what was paid for has been granted yet.
	const back = url.searchParams.get('checkout');
	const session = url.searchParams.get('session');
	const granted =
		back === 'plan'
			? hasPlan(account)
			: back === 'pack'
				? !!account?.credits.some((credit) => credit.source === session)
				: null;
	return {
		email: locals.user.email,
		back: granted === null ? null : { what: back, granted },
		usage: hasPlan(account) ? usage(account, Date.now()) : null,
		/** Extra credits bought, plan or not: a pack outlives the plan it was bought with. */
		extra: account ? usage(account, Date.now()).credits.extra : 0,
		extraPastLimits: account?.extraPastLimits ?? false,
		selling: !!billing,
		billingError,
		offers: catalog && {
			plan: {
				name: catalog.plan.name,
				description: catalog.plan.description,
				price: price(catalog.plan.amount, catalog.plan.currency),
				launch: catalog.plan.offer.launch
			},
			pack: {
				name: catalog.pack.name,
				description: catalog.pack.description,
				price: price(catalog.pack.amount, catalog.pack.currency),
				launch: false
			}
		},
		subscription
	};
};

function price(amount: number | null, currency: string): string {
	if (amount === null) return '';
	return new Intl.NumberFormat('en-US', {
		style: 'currency',
		currency,
		minimumFractionDigits: amount % 100 ? 2 : 0
	}).format(amount / 100);
}

/** Checkout or the portal: Stripe's pages, which a plain form's redirect goes on to. */
async function goToStripe(open: () => Promise<string | null>) {
	let url: string | null;
	try {
		url = await open();
	} catch (err) {
		if (err instanceof BillingError) return fail(400, { error: err.message });
		console.error('[nolune api] Stripe:', err);
		return fail(502, { error: "Stripe can't be reached just now. Try again in a little while." });
	}
	if (!url) return fail(400, { error: 'This account has paid for nothing yet.' });
	redirect(303, url);
}

export const actions: Actions = {
	subscribe: async ({ locals }) => {
		if (!locals.user) redirect(303, '/sign-in');
		const { billing } = await getService();
		if (!billing) return fail(404, { error: "The plan isn't on sale yet." });
		const person = locals.user;
		return goToStripe(() => billing.billing.checkout(person, 'plan'));
	},
	buyCredits: async ({ locals }) => {
		if (!locals.user) redirect(303, '/sign-in');
		const { billing } = await getService();
		if (!billing) return fail(404, { error: "Extra credits aren't on sale yet." });
		const person = locals.user;
		return goToStripe(() => billing.billing.checkout(person, 'pack'));
	},
	manage: async ({ locals }) => {
		if (!locals.user) redirect(303, '/sign-in');
		const { billing } = await getService();
		if (!billing) return fail(404, { error: 'There is nothing to manage yet.' });
		const userId = locals.user.id;
		return goToStripe(() => billing.billing.portal(userId));
	},
	extraCredits: async ({ locals, request }) => {
		if (!locals.user) redirect(303, '/sign-in');
		const on = (await request.formData()).get('on') === 'true';
		const { db } = await getService();
		await updateAccount(db, locals.user.id, (account) =>
			account ? { ...account, extraPastLimits: on } : null
		);
	},
	signOut: async ({ request }) => {
		const { auth } = await getService();
		await auth.api.signOut({ headers: request.headers });
		redirect(303, '/sign-in');
	}
};
