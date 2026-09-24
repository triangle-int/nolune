import { fail, redirect } from '@sveltejs/kit';
import { APIError } from 'better-auth/api';
import { getAuth } from '$lib/server/auth';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	if (locals.user) redirect(303, '/');
};

export const actions: Actions = {
	default: async ({ request }) => {
		const form = await request.formData();
		const email = form.get('email')?.toString().trim().toLowerCase() ?? '';
		const password = form.get('password')?.toString() ?? '';
		try {
			await getAuth().api.signInEmail({ body: { email, password }, headers: request.headers });
		} catch (err) {
			if (err instanceof APIError) {
				return fail(err.status === 'TOO_MANY_REQUESTS' ? 429 : 400, {
					email,
					message:
						err.status === 'TOO_MANY_REQUESTS'
							? 'Too many attempts. Wait a minute and try again.'
							: 'Wrong email or password.'
				});
			}
			throw err;
		}
		redirect(303, '/');
	}
};
