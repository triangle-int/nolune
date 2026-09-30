import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, beforeEach } from 'vitest';

/**
 * Runs before every test file (`test.setupFiles` in each package's vitest config). Each file gets
 * its own nolune home in a temp folder, emptied before every test, so a test starts with no
 * database and no profile folders, and nothing touches ~/.nolune. paths.ts reads NOLUNE_HOME when
 * it's first imported, which is after this.
 */
const home = mkdtempSync(join(tmpdir(), 'nolune-test-'));
process.env.NOLUNE_HOME = home;
process.env.NOLUNE_GLOBAL_SKILLS = join(home, 'global-skills');

/** getDb() opens the database again, migrating a fresh file, on its next call. */
function closeDb(): void {
	const holder = globalThis as unknown as { __noluneDb?: { $client: { close(): void } } };
	holder.__noluneDb?.$client.close();
	delete holder.__noluneDb;
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
