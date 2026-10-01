import { mkdtempSync, readFileSync, readdirSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { initConfig } from '@nolune/core';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { runCli } from './run.ts';
import { testIo } from './test/io.ts';

/** Plays Firecrawl, without a key: its free tier. */
let server: Server;
let env: Record<string, string>;

const LONG = Array.from({ length: 800 }, (_, i) => `Line ${i}: ${'word '.repeat(6).trim()}`).join(
	'\n'
);

function answer(path: string, body: Record<string, unknown>): [number, object] {
	if (path === '/v2/search') {
		if (body.query === 'too many') {
			return [429, { success: false, error: 'Daily limit reached' }];
		}
		if (body.query === 'nothing') return [200, { success: true, data: { web: [] } }];
		return [
			200,
			{
				success: true,
				data: {
					web: [
						{
							title: 'Apotheke am Markt',
							url: 'https://apotheke.example/',
							description: 'Open Sundays.'
						},
						{ title: 'Notdienst', url: 'https://notdienst.example/' }
					]
				}
			}
		];
	}
	const page = body.url as string;
	const markdown = page.includes('long') ? LONG : page.includes('empty') ? '' : 'Sunday 10–18';
	return [
		200,
		{
			success: true,
			data: {
				markdown,
				metadata: {
					title: 'Opening hours',
					sourceURL: page,
					statusCode: page.includes('gone') ? 404 : 200
				}
			}
		}
	];
}

beforeAll(async () => {
	server = createServer((req, res) => {
		let raw = '';
		req.on('data', (chunk) => (raw += chunk));
		req.on('end', () => {
			const [status, json] = answer(req.url ?? '', JSON.parse(raw || '{}'));
			res.writeHead(status, { 'content-type': 'application/json' });
			res.end(JSON.stringify(json));
		});
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
	env = { FIRECRAWL_API_URL: url, TMPDIR: mkdtempSync(join(tmpdir(), 'nolune-web-test-')) };
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	initConfig();
	vi.stubEnv('FIRECRAWL_API_KEY', '');
});

async function run(argv: string[], cwd = '/nonexistent') {
	const { io, out, err } = testIo({ env, cwd });
	const code = await runCli(argv, io);
	return { code, out: out(), err: err() };
}

describe('nolune web search', () => {
	it('prints the results, numbered', async () => {
		expect(await run(['web', 'search', 'pharmacy', 'open', 'sunday'])).toEqual({
			code: 0,
			out:
				'1. Apotheke am Markt\n   https://apotheke.example/\n   Open Sundays.\n\n' +
				'2. Notdienst\n   https://notdienst.example/\n',
			err: ''
		});
		expect((await run(['web', 'search', 'nothing'])).out).toBe('No results for "nothing".\n');
	});

	it('refuses options it would have to guess at', async () => {
		expect((await run(['web', 'search'])).err).toContain('say what to look for');
		expect((await run(['web', 'search', 'x', '--limit', '50'])).err).toContain(
			'--limit is between 1 and 20.'
		);
		expect((await run(['web', 'search', 'x', '--recent', 'hour'])).err).toContain(
			'--recent is one of day, week, month, year.'
		);
		expect((await run(['web', 'search', 'x', '--country', 'germany'])).err).toContain(
			'--country is a two-letter country code'
		);
	});

	it('says where to add a key when the free tier is used up', async () => {
		const used = await run(['web', 'search', 'too many']);
		expect(used.code).toBe(1);
		expect(used.err).toContain(
			"nolune: Firecrawl's free tier has had enough from this computer for today (Daily limit reached). An admin can add one under Models & keys"
		);
		// Nothing is wrong without a key: `nolune config` says so.
		expect((await run(['config'])).out).toContain(
			'\nfirecrawl     no key: the free tier, limited per day (nolune key set firecrawl)\n'
		);
	});
});

describe('nolune web read', () => {
	it('prints a page with its title and address', async () => {
		expect(await run(['web', 'read', 'apotheke.example/hours'])).toEqual({
			code: 0,
			out: '# Opening hours\nhttps://apotheke.example/hours\n\nSunday 10–18\n',
			err: ''
		});
	});

	it('saves a long page whole and prints its start', async () => {
		const { code, out } = await run(['web', 'read', 'https://long.example/']);
		expect(code).toBe(0);
		const note =
			/\n\n\[(\d+) more characters\. The whole page is in (.+\.md): grep it, or read on with sed -n\.\]\n$/.exec(
				out
			);
		expect(note).not.toBeNull();
		const [, rest, file] = note!;
		expect(file.startsWith(join(env.TMPDIR, 'nolune-web'))).toBe(true);
		expect(file).toMatch(/-long-example\.md$/);
		const whole = readFileSync(file, 'utf8');
		expect(whole).toBe(`# Opening hours\nhttps://long.example/\n\n${LONG}\n`);
		// Cut at the end of a line, within what a command's output keeps.
		const shown = out.slice(0, note!.index);
		expect(shown.length).toBeLessThanOrEqual(20_000);
		expect(whole.startsWith(`${shown}\n`)).toBe(true);
		expect(shown.length + Number(rest)).toBe(whole.length);
	});

	it('saves the page where --out says', async () => {
		const dir = mkdtempSync(join(tmpdir(), 'nolune-web-out-'));
		const saved = await run(
			['web', 'read', 'https://long.example/', '--out', 'pages/hours.md'],
			dir
		);
		const file = join(dir, 'pages', 'hours.md');
		expect(saved.out).toMatch(
			/^Saved https:\/\/long\.example\/ \(\d+ characters\) to .+hours\.md\n$/
		);
		expect(readFileSync(file, 'utf8')).toContain(LONG);
		expect(readdirSync(join(dir, 'pages'))).toEqual(['hours.md']);
	});

	it('says when a page answered with an error, or has no text', async () => {
		expect((await run(['web', 'read', 'https://gone.example/'])).out).toBe(
			'The page answered 404; this is what it showed.\n# Opening hours\nhttps://gone.example/\n\nSunday 10–18\n'
		);
		expect((await run(['web', 'read', 'https://empty.example/'])).out).toBe(
			'https://empty.example/ has no text Firecrawl could read.\n'
		);
		expect((await run(['web', 'read', 'file:///etc/passwd'])).err).toContain(
			'Only web pages can be read'
		);
	});
});
