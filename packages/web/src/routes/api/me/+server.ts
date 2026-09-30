import { error, json } from '@sveltejs/kit';
import { MAX_NAME_LENGTH, UserNameError, renameUser } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireUser } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Changes the signed-in person's name: `{ name }`. */
export const PATCH: RequestHandler = async ({ locals, request }) => {
	const user = requireUser(locals);
	const { m } = translations(locals.locale);
	const body = (await request.json().catch(() => null)) as { name?: unknown } | null;
	const name = typeof body?.name === 'string' ? body.name : '';
	try {
		return json({ name: renameUser(user.id, name) });
	} catch (err) {
		if (!(err instanceof UserNameError)) throw err;
		const problems = {
			required: m.account.nameRequired,
			tooLong: m.account.nameTooLong(MAX_NAME_LENGTH),
			email: m.account.nameHasAt,
			taken: m.account.nameTaken(name.trim())
		};
		error(400, problems[err.reason]);
	}
};
