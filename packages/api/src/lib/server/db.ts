import { drizzle } from 'drizzle-orm/node-postgres';
import { migrate } from 'drizzle-orm/node-postgres/migrator';
import type { PgDatabase, PgQueryResultHKT } from 'drizzle-orm/pg-core';
import pg from 'pg';
import * as schema from './schema.ts';

export { schema };

/** Postgres through any driver: node-postgres in the service, PGlite in tests. */
export type Db = PgDatabase<PgQueryResultHKT, typeof schema>;

/** Migrations drizzle-kit wrote from schema.ts, next to the build (or the package, in dev). */
export const MIGRATIONS = process.env.MIGRATIONS_DIR || 'drizzle';

/** A pool for `url` (DATABASE_URL), with the schema migrated before it's used. */
export async function connect(url: string): Promise<Db> {
	const db = drizzle(new pg.Pool({ connectionString: url }), { schema });
	await migrate(db, { migrationsFolder: MIGRATIONS });
	return db;
}
