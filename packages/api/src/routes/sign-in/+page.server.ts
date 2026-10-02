import { fail, redirect } from '@sveltejs/kit';
import { APIError } from 'better-auth/api';
import { signInCodeEmail } from '$lib/server/email';
import { codesByEmail, codesByNetwork, signInsByNetwork } from '$lib/server/rate-limit';
import { getService } from '$lib/server/service';
import type { Actions, PageServerLoad } from './$types';

/** Where to go once signed in (`?next=`, which the forms carry on): a page of this site only. */
function next(to: string | null | undefined): string {
	return to?.startsWith('/') && !to.startsWith('//') ? to : '/';
}

function email(form: FormData): string {
	return form.get('email')?.toString().trim().toLowerCase() ?? '';
}

export const load: PageServerLoad = ({ locals, url }) => {
	if (locals.user) redirect(303, next(url.searchParams.get('next')));
};

export const actions: Actions = {
	send: async ({ request, getClientAddress }) => {
		const address = email(await request.formData());
		if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(address)) {
			return fail(400, { email: address, message: "That doesn't look like an email address." });
		}
		if (!codesByNetwork.allow(getClientAddress()) || !codesByEmail.allow(address)) {
			return fail(429, {
				email: address,
				message: 'Too many codes asked for. Try again in a few minutes.'
			});
		}
		const { auth, sendEmail, mayJoin } = await getService();
		if (!mayJoin(address)) {
			return fail(403, {
				email: address,
				message: "The nolune plan isn't open yet: only invited addresses can sign in for now."
			});
		}
		// The code is made here and sent by this action rather than better-auth's own, which logs a
		// failure to send and says it went: the page should say when it didn't.
		const otp = await auth.api.createVerificationOTP({ body: { email: address, type: 'sign-in' } });
		try {
			await sendEmail(signInCodeEmail(address, otp));
		} catch (err) {
			console.error('[nolune api] a sign-in code could not be sent:', err);
			return fail(502, {
				email: address,
				message: "The code couldn't be sent just now. Try again in a few minutes."
			});
		}
		return { email: address, sent: true };
	},

	verify: async ({ request, getClientAddress }) => {
		const form = await request.formData();
		const address = email(form);
		const otp = form.get('code')?.toString().replace(/\D/g, '') ?? '';
		if (!signInsByNetwork.allow(getClientAddress())) {
			return fail(429, {
				email: address,
				sent: true,
				message: 'Too many tries. Wait a few minutes.'
			});
		}
		const { auth } = await getService();
		try {
			await auth.api.signInEmailOTP({ body: { email: address, otp }, headers: request.headers });
		} catch (err) {
			if (!(err instanceof APIError)) throw err;
			return fail(400, {
				email: address,
				sent: true,
				message: "That code isn't right, or it has run out. Check the email, or send a new one."
			});
		}
		redirect(303, next(form.get('next')?.toString()));
	}
};
