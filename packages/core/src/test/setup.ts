import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeEach } from 'vitest';

/**
 * Runs before every test file (`test.setupFiles` in vite.config.ts). Each file gets its own btw
 * home in a temp folder, emptied before every test, so a test starts with no database and no
 * profile folders, and nothing touches ~/.btw-agent. paths.ts reads BTW_HOME when it's first
 * imported, which is after this.
 */
const home = mkdtempSync(join(tmpdir(), 'btw-test-'));
process.env.BTW_HOME = home;
process.env.BTW_GLOBAL_SKILLS = join(home, 'global-skills');

/** getDb() opens the database again, migrating a fresh file, on its next call. */
function closeDb(): void {
	const holder = globalThis as unknown as { __btwDb?: { $client: { close(): void } } };
	holder.__btwDb?.$client.close();
	delete holder.__btwDb;
}

beforeEach(() => {
	closeDb();
	rmSync(home, { recursive: true, force: true });
	mkdirSync(home);
});

afterAll(() => {
	closeDb();
	rmSync(home, { recursive: true, force: true });
});
