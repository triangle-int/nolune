import { error, fail, type ActionFailure } from '@sveltejs/kit';
import {
	INVITE_DAYS,
	createInvite,
	createUser,
	deleteUser,
	findUserById,
	generatePassword,
	listInvites,
	listUsers,
	publicOrigin,
	readConfig,
	revokeInvite,
	setAdmin,
	setPassword
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { pictureUrl } from '$lib/pictures';
import { requireAdmin } from '$lib/server/access';
import { accountProblem } from '$lib/server/accounts';
import type { Actions, PageServerLoad } from './$types';

/** The address people open: the relay's, or the one set, as `nolune setup` printed it. */
function address(): string {
	return publicOrigin(readConfig());
}

export const load: PageServerLoad = ({ locals }) => {
	const me = requireAdmin(locals);
	return {
		me: me.id,
		people: listUsers().map((u) => ({ ...u, picture: pictureUrl(u.picture) })),
		// Only the ones that still work; never their links, which aren't kept.
		invites: listInvites(),
		inviteDays: INVITE_DAYS
	};
};

/**
 * The account a row's form is about, and the admin sending it: someone else's, for what could
 * lock the admin out.
 */
async function someone(
	locals: App.Locals,
	request: Request,
	notMine = false
): Promise<
	| { form: FormData; found: NonNullable<ReturnType<typeof findUserById>> }
	| ActionFailure<{ peopleError: string }>
> {
	const me = requireAdmin(locals);
	const form = await request.formData();
	const { m } = translations(locals.locale);
	const found = findUserById(form.get('id')?.toString() ?? '');
	if (!found) return fail(404, { peopleError: m.people.gone });
	if (notMine && found.id === me.id) return fail(400, { peopleError: m.people.notYourself });
	return { form, found };
}

export const actions: Actions = {
	/** An account with a password nolune makes, shown this once. */
	create: async ({ locals, request }) => {
		requireAdmin(locals);
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const name = form.get('name')?.toString() ?? '';
		const email = form.get('email')?.toString() ?? '';
		const isAdmin = form.get('admin') === 'on';
		const password = generatePassword();
		try {
			await createUser({ name, email, password, isAdmin });
		} catch (err) {
			const problem = accountProblem(err, m, name);
			if (problem === null) throw err;
			return fail(400, { createError: problem, name, email, isAdmin });
		}
		return {
			created: {
				name: name.trim(),
				email: email.trim().toLowerCase(),
				password,
				address: address()
			}
		};
	},
	invite: async ({ locals, request }) => {
		const me = requireAdmin(locals);
		const name = (await request.formData()).get('name')?.toString() ?? '';
		const { invite, token } = createInvite({ name, createdBy: me.id });
		return { invited: { name: invite.name, link: `${address()}/invite/${token}` } };
	},
	revokeInvite: async ({ locals, request }) => {
		requireAdmin(locals);
		revokeInvite((await request.formData()).get('id')?.toString() ?? '');
		return { inviteMessage: translations(locals.locale).m.people.revoked };
	},
	/** A new password nolune makes, shown this once. */
	password: async ({ locals, request }) => {
		const who = await someone(locals, request);
		if (!('found' in who)) return who;
		const { found } = who;
		const password = generatePassword();
		await setPassword(found.email, password);
		return { reset: { id: found.id, name: found.name, password } };
	},
	admin: async ({ locals, request }) => {
		const who = await someone(locals, request, true);
		if (!('found' in who)) return who;
		const { form, found } = who;
		const on = form.get('on')?.toString();
		if (on !== 'true' && on !== 'false') error(400, 'Say whether they should be an admin');
		setAdmin(found.email, on === 'true');
		const { m } = translations(locals.locale);
		return {
			peopleMessage:
				on === 'true' ? m.people.nowAdmin(found.name) : m.people.noLongerAdmin(found.name)
		};
	},
	remove: async ({ locals, request }) => {
		const who = await someone(locals, request, true);
		if (!('found' in who)) return who;
		const { found } = who;
		deleteUser(found.email);
		return { peopleMessage: translations(locals.locale).m.people.removed(found.name) };
	}
};
