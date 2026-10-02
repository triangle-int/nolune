import { error, json } from '@sveltejs/kit';
import { readEvent } from '$lib/server/billing';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/*
 * Stripe's events (DESIGN.md, Payments): what was paid for is granted here. A `400` is for an
 * event that isn't Stripe's; a `500` has Stripe send it again, for up to three days.
 */
export const POST: RequestHandler = async ({ request }) => {
	const { billing } = await getService();
	if (!billing) error(404, 'Not Found');
	let event;
	try {
		event = readEvent(
			billing.stripe,
			await request.text(),
			request.headers.get('stripe-signature'),
			billing.webhookSecret
		);
	} catch {
		error(400, "That isn't an event from Stripe");
	}
	await billing.billing.handle(event);
	return json({ received: true });
};
