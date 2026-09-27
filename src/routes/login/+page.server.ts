import { fail, redirect } from '@sveltejs/kit';
import { APIError } from 'better-auth/api';
import { translations } from '$lib/i18n';
import { getAuth } from '$lib/server/auth';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	if (locals.user) redirect(303, '/');
};

export const actions: Actions = {
	default: async ({ request, locals }) => {
		const form = await request.formData();
		const email = form.get('email')?.toString().trim().toLowerCase() ?? '';
		const password = form.get('password')?.toString() ?? '';
		try {
			await getAuth().api.signInEmail({ body: { email, password }, headers: request.headers });
		} catch (err) {
			if (err instanceof APIError) {
				const { m } = translations(locals.locale);
				return fail(err.status === 'TOO_MANY_REQUESTS' ? 429 : 400, {
					email,
					message:
						err.status === 'TOO_MANY_REQUESTS' ? m.login.tooManyAttempts : m.login.wrongPassword
				});
			}
			throw err;
		}
		redirect(303, '/');
	}
};
