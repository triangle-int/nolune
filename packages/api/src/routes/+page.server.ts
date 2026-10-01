import { redirect } from '@sveltejs/kit';
import { readAccount } from '$lib/server/accounts';
import { usage } from '$lib/server/limits';
import { getService } from '$lib/server/service';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	if (!locals.user) redirect(303, '/sign-in');
	const { db } = await getService();
	const account = await readAccount(db, locals.user.id);
	return {
		email: locals.user.email,
		usage: account && account.limits.week > 0 ? usage(account, Date.now()) : null
	};
};

export const actions: Actions = {
	signOut: async ({ request }) => {
		const { auth } = await getService();
		await auth.api.signOut({ headers: request.headers });
		redirect(303, '/sign-in');
	}
};
