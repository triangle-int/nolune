import { is } from 'drizzle-orm';
import { getTableConfig, SQLiteTable } from 'drizzle-orm/sqlite-core';
import { expect, it } from 'vitest';
import { getDb, schema } from './index.ts';

const tables = Object.values(schema).filter((value) => is(value, SQLiteTable));

it('migrates a new database to exactly the tables and columns in schema.ts', () => {
	const client = getDb().$client;
	const migrated = client
		.prepare(
			`select name from sqlite_master where type = 'table' and name not like 'sqlite_%' and name not like '__drizzle%'`
		)
		.pluck()
		.all();
	expect(migrated.sort()).toEqual(tables.map((table) => getTableConfig(table).name).sort());

	for (const table of tables) {
		const { name, columns } = getTableConfig(table);
		const found = client.pragma(`table_info(${name})`) as { name: string; notnull: number }[];
		expect(found.map((c) => [c.name, !!c.notnull]).sort(), name).toEqual(
			columns.map((c) => [c.name, c.notNull]).sort()
		);
	}
});

it('enforces foreign keys and lets the gateway and the CLI share the file', () => {
	const client = getDb().$client;
	expect(client.pragma('foreign_keys', { simple: true })).toBe(1);
	expect(client.pragma('journal_mode', { simple: true })).toBe('wal');
});
