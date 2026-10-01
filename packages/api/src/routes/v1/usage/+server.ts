import { json } from '@sveltejs/kit';
import { readAccount } from '$lib/server/accounts';
import { usage } from '$lib/server/limits';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** How much of each limit a gateway's plan has used (`x-nolune-usage` carries the same). */
export const GET: RequestHandler = async ({ locals }) => {
	if (!locals.user) {
		return json(
			{ error: { code: 'not_signed_in', message: 'Sign in to the nolune plan again.' } },
			{ status: 401 }
		);
	}
	const { db } = await getService();
	const account = await readAccount(db, locals.user.id);
	return json({ email: locals.user.email, usage: account ? usage(account, Date.now()) : null });
};
