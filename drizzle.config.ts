import { homedir } from 'node:os';
import { join } from 'node:path';
import { defineConfig } from 'drizzle-kit';

export default defineConfig({
	schema: './packages/core/src/db/schema.ts',
	out: './packages/core/drizzle',
	dialect: 'sqlite',
	dbCredentials: {
		url: join(process.env.NOLUNE_HOME || join(homedir(), '.nolune'), 'nolune.db')
	},
	verbose: true,
	strict: true
});
