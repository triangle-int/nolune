import { sql } from 'drizzle-orm';
import { getService } from '$lib/server/service';
import type { RequestHandler } from './$types';

/** `ok` while the API and its Postgres answer: for the container's health check and check.sh. */
export const GET: RequestHandler = async () => {
	try {
		const { db } = await getService();
		await db.execute(sql`select 1`);
		return new Response('ok', { headers: { 'cache-control': 'no-store' } });
	} catch {
		return new Response('Postgres is not answering', { status: 503 });
	}
};
