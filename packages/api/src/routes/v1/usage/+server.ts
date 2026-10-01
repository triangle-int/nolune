import { json } from '@sveltejs/kit';
import { readAccount } from '$lib/server/accounts';
import { hasPlan, usage } from '$lib/server/limits';
import { notSignedIn } from '$lib/server/proxy';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** How much of each limit a gateway's plan has used (`x-nolune-usage` carries the same). */
export const GET: RequestHandler = async ({ locals }) => {
	if (!locals.user) return notSignedIn();
	const { db } = await getService();
	const account = await readAccount(db, locals.user.id);
	return json({
		email: locals.user.email,
		usage: hasPlan(account) ? usage(account, Date.now()) : null
	});
};
