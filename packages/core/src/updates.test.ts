import { existsSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { initConfig, updateConfig } from './config.ts';
import { paths } from './paths.ts';
import {
	NOLUNE_VERSION,
	availableUpdate,
	checkForUpdate,
	describeUpdates,
	forgetRelease,
	installKind,
	isNewer,
	onReleaseFound,
	savedRelease
} from './updates.ts';

/** One past the running version, so it's always the newer one. */
const NEXT = `${Number(NOLUNE_VERSION.split('.')[0]) + 1}.0.0`;
const PAGE = 'https://github.com/triangle-int/nolune/releases';

function release(version: string, extra: Record<string, unknown> = {}) {
	return {
		tag_name: `v${version}`,
		html_url: `${PAGE}/tag/v${version}`,
		published_at: '2026-10-01T09:00:00Z',
		assets: [
			{
				name: 'nolune-macos-apple-silicon.dmg',
				browser_download_url: `${PAGE}/download/v${version}/nolune-macos-apple-silicon.dmg`
			},
			{
				name: 'nolune-macos-intel.dmg',
				browser_download_url: `${PAGE}/download/v${version}/nolune-macos-intel.dmg`
			}
		],
		...extra
	};
}

/** What GitHub answers next: a status and a body. */
let answer: [number, unknown] = [200, release(NEXT)];
const seen: { accept?: string; agent?: string }[] = [];
let server: Server;
let url: string;

beforeAll(async () => {
	server = createServer((req, res) => {
		seen.push({ accept: req.headers.accept, agent: req.headers['user-agent'] });
		res.writeHead(answer[0], { 'content-type': 'application/json' });
		res.end(JSON.stringify(answer[1]));
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	url = `http://127.0.0.1:${(server.address() as AddressInfo).port}/releases/latest`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	answer = [200, release(NEXT)];
	seen.length = 0;
});

describe('isNewer', () => {
	it.each([
		['0.4.0', '0.3.0', true],
		['0.10.0', '0.9.9', true],
		['1.0.0', '0.99.0', true],
		['v0.3.1', '0.3.0', true],
		['0.3.0', '0.3.0', false],
		['0.3.0', '0.4.0', false],
		['0.3.0', 'v0.3.0', false]
	])('%s after %s: %s', (a, b, newer) => {
		expect(isNewer(a, b)).toBe(newer);
	});
});

describe('checkForUpdate', () => {
	it("saves GitHub's latest release, with the app's disk images", async () => {
		const found = await checkForUpdate(url);
		expect(found).toEqual({
			version: NEXT,
			url: `${PAGE}/tag/v${NEXT}`,
			publishedAt: '2026-10-01T09:00:00Z',
			downloads: {
				arm64: `${PAGE}/download/v${NEXT}/nolune-macos-apple-silicon.dmg`,
				x64: `${PAGE}/download/v${NEXT}/nolune-macos-intel.dmg`
			}
		});
		expect(savedRelease()?.latest).toEqual(found);
		expect(seen[0]).toEqual({
			accept: 'application/vnd.github+json',
			agent: `nolune/${NOLUNE_VERSION}`
		});
	});

	it('leaves out links to anywhere but nolune’s releases', async () => {
		answer = [
			200,
			release(NEXT, {
				assets: [
					{
						name: 'nolune-macos-apple-silicon.dmg',
						browser_download_url: 'https://example.com/nolune.dmg'
					}
				]
			})
		];
		expect((await checkForUpdate(url))?.downloads).toEqual({});

		answer = [200, release(NEXT, { html_url: 'https://example.com/nolune' })];
		await expect(checkForUpdate(url)).rejects.toThrow('no release');
	});

	it('saves that there is none yet when GitHub has none', async () => {
		answer = [404, { message: 'Not Found' }];
		expect(await checkForUpdate(url)).toBeNull();
		expect(savedRelease()).toMatchObject({ latest: null });
	});

	it('keeps what it heard before when GitHub fails or says something else', async () => {
		await checkForUpdate(url);
		const before = savedRelease();

		answer = [403, { message: 'API rate limit exceeded' }];
		await expect(checkForUpdate(url)).rejects.toThrow('GitHub answered 403');
		answer = [200, { tag_name: 'nightly', html_url: `${PAGE}/tag/nightly` }];
		await expect(checkForUpdate(url)).rejects.toThrow('no release');

		expect(savedRelease()).toEqual(before);
	});

	it('tells listeners when the release is another one, and only then', async () => {
		let found = 0;
		const stop = onReleaseFound(() => found++);
		await checkForUpdate(url);
		await checkForUpdate(url);
		expect(found).toBe(1);
		answer = [200, release(`${NEXT.split('.')[0]}.1.0`)];
		await checkForUpdate(url);
		expect(found).toBe(2);
		stop();
	});
});

describe('availableUpdate', () => {
	it('is a newer release, with how this install updates', async () => {
		await checkForUpdate(url);
		expect(availableUpdate()).toEqual({
			current: NOLUNE_VERSION,
			version: NEXT,
			url: `${PAGE}/tag/v${NEXT}`,
			// The tests run from a checkout.
			install: 'source',
			command: 'git pull && pnpm install && pnpm build',
			download: null
		});
		expect(installKind()).toBe('source');
	});

	it('is nothing when the running nolune is the newest', async () => {
		answer = [200, release(NOLUNE_VERSION)];
		await checkForUpdate(url);
		expect(availableUpdate()).toBeNull();
		expect(describeUpdates()).toMatch(new RegExp(`^${NOLUNE_VERSION}, the newest \\(checked `));
	});

	it('is nothing while the checks are off', async () => {
		initConfig();
		await checkForUpdate(url);
		updateConfig((c) => (c.updateCheck = false));
		expect(availableUpdate()).toBeNull();
		expect(describeUpdates()).toBe(`${NOLUNE_VERSION} (not checking for new releases)`);
		forgetRelease();
		expect(existsSync(paths.latestRelease)).toBe(false);
	});

	it('says how to update, for `nolune config`', async () => {
		expect(describeUpdates()).toBe(`${NOLUNE_VERSION} (not checked for new releases yet)`);
		await checkForUpdate(url);
		expect(describeUpdates()).toBe(
			`${NOLUNE_VERSION}; ${NEXT} is out (${PAGE}/tag/v${NEXT}). Update: git pull && pnpm install && pnpm build`
		);
	});
});
