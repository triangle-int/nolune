import { redirect } from '@sveltejs/kit';
import { getAuth } from '$lib/server/auth';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ request }) => {
	await getAuth().api.signOut({ headers: request.headers });
	redirect(303, '/login');
};
