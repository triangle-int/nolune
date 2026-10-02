import { fileURLToPath } from 'node:url';
import { PGlite } from '@electric-sql/pglite';
import { drizzle } from 'drizzle-orm/pglite';
import { migrate } from 'drizzle-orm/pglite/migrator';
import { schema, type Db } from '../db.ts';

/** Postgres in this process (PGlite), migrated like the service's: a fresh one for each test. */
export async function testDb(): Promise<Db> {
	const db = drizzle(new PGlite(), { schema });
	await migrate(db, {
		migrationsFolder: fileURLToPath(new URL('../../../../drizzle', import.meta.url))
	});
	return db;
}

/** A person to hang a plan on. */
export async function addUser(db: Db, id = 'u1', email = `${id}@example.com`): Promise<string> {
	await db.insert(schema.user).values({ id, name: '', email });
	return id;
}
