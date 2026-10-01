import { fail, redirect } from '@sveltejs/kit';
import { APIError } from 'better-auth/api';
import { getService } from '$lib/server/service';
import type { Actions, PageServerLoad } from './$types';

/*
 * Linking a gateway: `nolune nolune-plan setup` (or Models & keys) shows a code and this page's
 * address. Whoever opens it signs in, sees the code, and approves it; the gateway, asking every
 * few seconds, then gets its token. Showing the code to someone signed in is what makes it theirs
 * to approve (better-auth's device authorization).
 */

type Status = 'pending' | 'approved' | 'denied' | 'unknown';

export const load: PageServerLoad = async ({ locals, url, request }) => {
	if (!locals.user) {
		redirect(303, `/sign-in?next=${encodeURIComponent(url.pathname + url.search)}`);
	}
	const code = url.searchParams.get('user_code')?.trim() ?? '';
	if (!code) return { code, status: null };
	const { auth } = await getService();
	try {
		const shown = await auth.api.deviceVerify({
			query: { user_code: code },
			headers: request.headers
		});
		return { code, status: shown.status as Status };
	} catch (err) {
		if (!(err instanceof APIError)) throw err;
		return { code, status: 'unknown' as Status };
	}
};

async function decide(request: Request, approve: boolean) {
	const userCode = (await request.formData()).get('code')?.toString() ?? '';
	const { auth } = await getService();
	try {
		if (approve) await auth.api.deviceApprove({ body: { userCode }, headers: request.headers });
		else await auth.api.deviceDeny({ body: { userCode }, headers: request.headers });
	} catch (err) {
		if (!(err instanceof APIError)) throw err;
		return fail(400, {
			message: 'This code has run out or was used already. Ask nolune for a new one.'
		});
	}
	return { done: approve ? ('approved' as const) : ('denied' as const) };
}

export const actions: Actions = {
	approve: ({ request }) => decide(request, true),
	deny: ({ request }) => decide(request, false)
};
