import { mkdirSync } from 'node:fs';
import Database from 'better-sqlite3';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import { migrate } from 'drizzle-orm/better-sqlite3/migrator';
import { paths } from '../paths.ts';
import * as schema from './schema.ts';

function open() {
	mkdirSync(paths.home, { recursive: true });
	const client = new Database(paths.db);
	// The gateway and the CLI open the same file.
	client.pragma('journal_mode = WAL');
	client.pragma('busy_timeout = 5000');
	client.pragma('foreign_keys = ON');
	const db = drizzle(client, { schema });
	migrate(db, { migrationsFolder: paths.migrations });
	return db;
}

export type DB = ReturnType<typeof open>;

const holder = globalThis as unknown as { __btwDb?: DB };

export function getDb(): DB {
	holder.__btwDb ??= open();
	return holder.__btwDb;
}

export { schema };
