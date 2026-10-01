import { fail, redirect } from '@sveltejs/kit';
import { APIError } from 'better-auth/api';
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
		const { auth } = await getService();
		await auth.api.sendVerificationOTP({ body: { email: address, type: 'sign-in' } });
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
