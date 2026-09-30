import { fail, redirect } from '@sveltejs/kit';
import { InviteError, MIN_PASSWORD_LENGTH, acceptInvite, findInvite } from '@nolune/core';
import { translations } from '$lib/i18n';
import { accountProblem } from '$lib/server/accounts';
import { getAuth } from '$lib/server/auth';
import type { Actions, PageServerLoad } from './$types';

/*
 * Where an invite link opens, signed in or not (hooks.server.ts lets `/invite/` through): the
 * person picks their name, email and password, and is signed in with them.
 */

export const load: PageServerLoad = ({ locals, params, setHeaders }) => {
	// The token is in the address: nothing this page links to gets it.
	setHeaders({ 'referrer-policy': 'no-referrer' });
	const invite = findInvite(params.token);
	return {
		invite: invite && {
			name: invite.name,
			createdBy: invite.createdBy,
			expiresAt: invite.expiresAt
		},
		// An account can't be made while signed in: the link is someone else's.
		signedInAs: locals.user?.name ?? null,
		minPassword: MIN_PASSWORD_LENGTH
	};
};

export const actions: Actions = {
	default: async ({ locals, params, request }) => {
		if (locals.user) redirect(303, '/');
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const name = form.get('name')?.toString() ?? '';
		const email = form.get('email')?.toString() ?? '';
		const password = form.get('password')?.toString() ?? '';
		// Not the password: the form keeps what was typed, since the page doesn't reload.
		const refuse = (problem: string) => fail(400, { name, email, problem });
		if (password !== (form.get('again')?.toString() ?? '')) return refuse(m.invite.mismatch);
		try {
			await acceptInvite(params.token, { name, email, password });
		} catch (err) {
			if (err instanceof InviteError) return fail(410, { gone: true });
			const problem = accountProblem(err, m, name);
			if (problem === null) throw err;
			return refuse(problem);
		}
		await getAuth().api.signInEmail({
			body: { email: email.trim().toLowerCase(), password },
			headers: request.headers
		});
		redirect(303, '/');
	}
};
