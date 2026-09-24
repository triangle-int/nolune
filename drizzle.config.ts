import { homedir } from 'node:os';
import { join } from 'node:path';
import { defineConfig } from 'drizzle-kit';

export default defineConfig({
	schema: './packages/core/src/db/schema.ts',
	out: './packages/core/drizzle',
	dialect: 'sqlite',
	dbCredentials: {
		url: join(process.env.BTW_HOME || join(homedir(), '.btw-agent'), 'btw.db')
	},
	verbose: true,
	strict: true
});
