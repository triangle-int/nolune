import { error, json } from '@sveltejs/kit';
import { renameUser } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireUser } from '$lib/server/access';
import { accountProblem } from '$lib/server/accounts';
import type { RequestHandler } from './$types';

/** Changes the signed-in person's name: `{ name }`. */
export const PATCH: RequestHandler = async ({ locals, request }) => {
	const user = requireUser(locals);
	const body = (await request.json().catch(() => null)) as { name?: unknown } | null;
	const name = typeof body?.name === 'string' ? body.name : '';
	try {
		return json({ name: renameUser(user.id, name) });
	} catch (err) {
		const problem = accountProblem(err, translations(locals.locale).m, name);
		if (problem === null) throw err;
		error(400, problem);
	}
};
