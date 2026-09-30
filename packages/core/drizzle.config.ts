import { homedir } from 'node:os';
import { join } from 'node:path';
import { defineConfig } from 'drizzle-kit';

export default defineConfig({
	schema: './src/db/schema.ts',
	out: './drizzle',
	dialect: 'sqlite',
	dbCredentials: {
		url: join(process.env.NOLUNE_HOME || join(homedir(), '.nolune'), 'nolune.db')
	},
	verbose: true,
	strict: true
});
