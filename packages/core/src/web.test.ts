import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { saveApiKey } from './api-keys.ts';
import { initConfig } from './config.ts';
import { WebError, readWebPage, searchWeb, webUrl } from './web.ts';

/** Plays Firecrawl: answers by the query or the page asked for, and by the key it's given. */
let server: Server;
let url: string;
const seen: { path: string; key: string | null; body: Record<string, unknown> }[] = [];

function answer(path: string, body: Record<string, unknown>, key: string | null): [number, object] {
	if (key !== null && key !== 'fc-good') {
		return [401, { success: false, error: 'Unauthorized: Invalid token' }];
	}
	if (path === '/v2/search') {
		if (body.query === 'too many') {
			// As Firecrawl says it: how to send a key follows on the next lines.
			return [
				429,
				{
					success: false,
					error:
						"You've hit Firecrawl's keyless free tier rate limit. To continue now, create a free API key at https://firecrawl.dev/k/abc\n\nThen authenticate with:\nAuthorization: Bearer YOUR_API_KEY"
				}
			];
		}
		if (body.query === 'broke') return [402, { success: false, error: 'Payment required' }];
		if (body.query === 'nothing') return [200, { success: true, data: { web: [] } }];
		if ((body.sources as string[])[0] === 'news') {
			return [
				200,
				{
					success: true,
					data: {
						news: [
							{
								title: 'Pharmacies strike',
								url: 'https://news.example/strike',
								snippet: 'Closed today.',
								date: '2 hours ago'
							}
						]
					}
				}
			];
		}
		return [
			200,
			{
				success: true,
				data: {
					web: [
						{
							title: 'Apotheke am Markt',
							url: 'https://apotheke.example/',
							description: 'Open Sundays 10–18.'
						},
						{ url: 'https://untitled.example/' },
						{ title: 'No address' }
					]
				}
			}
		];
	}
	if (path === '/v2/scrape') {
		if (body.url === 'https://gone.example/') {
			return [
				200,
				{
					success: true,
					data: {
						markdown: 'Not found',
						metadata: { title: 'Gone', sourceURL: body.url, statusCode: 404 }
					}
				}
			];
		}
		return [
			200,
			{
				success: true,
				data: {
					markdown: '\n# Opening hours\n\nSunday 10–18\n',
					metadata: {
						title: ' Apotheke am Markt ',
						sourceURL: body.url,
						url: 'https://apotheke.example/hours',
						statusCode: 200
					}
				}
			}
		];
	}
	return [404, { success: false, error: 'Not found' }];
}

beforeAll(async () => {
	server = createServer((req, res) => {
		let raw = '';
		req.on('data', (chunk) => (raw += chunk));
		req.on('end', () => {
			const body = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
			const key = req.headers.authorization?.replace(/^Bearer /, '') ?? null;
			seen.push({ path: req.url ?? '', key, body });
			const [status, json] = answer(req.url ?? '', body, key);
			res.writeHead(status, { 'content-type': 'application/json' });
			res.end(JSON.stringify(json));
		});
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
	vi.stubEnv('FIRECRAWL_API_URL', url);
});

afterAll(() => {
	server.close();
	vi.unstubAllEnvs();
});

beforeEach(() => {
	initConfig();
	vi.stubEnv('FIRECRAWL_API_KEY', '');
	seen.length = 0;
});

describe('searchWeb', () => {
	it("searches on Firecrawl's free tier without a key", async () => {
		const { results } = await searchWeb('pharmacy open sunday');
		expect(results).toEqual([
			{
				title: 'Apotheke am Markt',
				url: 'https://apotheke.example/',
				text: 'Open Sundays 10–18.',
				date: null
			},
			// A result without a title goes by its address; one without an address is left out.
			{ title: 'https://untitled.example/', url: 'https://untitled.example/', text: '', date: null }
		]);
		// No Authorization header at all: that's what Firecrawl's free tier takes.
		expect(seen[0]).toMatchObject({
			path: '/v2/search',
			key: null,
			body: { query: 'pharmacy open sunday', limit: 5, sources: ['web'] }
		});
	});

	it("uses the key from Models & keys, else the command's own", async () => {
		// Set with `nolune env set` before Firecrawl had a place in Models & keys.
		await searchWeb('pharmacy', { env: { FIRECRAWL_API_KEY: 'fc-good' } });
		saveApiKey('firecrawl', 'fc-good');
		await searchWeb('pharmacy', { env: { FIRECRAWL_API_KEY: 'fc-old-and-deleted' } });
		expect(seen.map((s) => s.key)).toEqual(['fc-good', 'fc-good']);
	});

	it('asks for news, recent results and a country', async () => {
		const { results } = await searchWeb('apotheken streik', {
			news: true,
			recent: 'day',
			country: 'de',
			limit: 3
		});
		expect(results).toEqual([
			{
				title: 'Pharmacies strike',
				url: 'https://news.example/strike',
				text: 'Closed today.',
				date: '2 hours ago'
			}
		]);
		expect(seen[0].body).toMatchObject({
			sources: ['news'],
			tbs: 'qdr:d',
			country: 'DE',
			limit: 3
		});
	});

	it('says in words what went wrong', async () => {
		await expect(searchWeb('too many')).rejects.toThrow(
			"Firecrawl's free tier has had enough from this computer for today (You've hit Firecrawl's keyless free tier rate limit. To continue now, create a free API key at https://firecrawl.dev/k/abc). An admin can add one under Models & keys in nolune, or with `nolune key set firecrawl`. A free Firecrawl account comes with more."
		);
		saveApiKey('firecrawl', 'fc-good');
		await expect(searchWeb('too many')).rejects.toThrow('too many requests from this key');
		await expect(searchWeb('broke')).rejects.toThrow("The Firecrawl key's credits are used up");
		saveApiKey('firecrawl', 'fc-deleted');
		await expect(searchWeb('pharmacy')).rejects.toThrow(
			"Firecrawl didn't accept its API key. An admin can add one under Models & keys in nolune, or with `nolune key set firecrawl`."
		);
		await expect(searchWeb('pharmacy')).rejects.toBeInstanceOf(WebError);
	});

	it("says when it can't reach Firecrawl", async () => {
		await expect(
			searchWeb('pharmacy', { env: { FIRECRAWL_API_URL: 'http://127.0.0.1:1' } })
		).rejects.toThrow(/^Couldn't reach Firecrawl \(ECONNREFUSED\)\.$/);
	});

	it('stops waiting when the command is stopped', async () => {
		const stop = new AbortController();
		stop.abort(new Error('stopped'));
		await expect(searchWeb('pharmacy', { signal: stop.signal })).rejects.toThrow('stopped');
	});
});

describe('readWebPage', () => {
	it("reads a page's main content as Markdown", async () => {
		const page = await readWebPage('apotheke.example/hours');
		expect(page).toEqual({
			url: 'https://apotheke.example/hours',
			title: 'Apotheke am Markt',
			markdown: '# Opening hours\n\nSunday 10–18',
			status: null,
			warning: null
		});
		expect(seen[0]).toMatchObject({
			path: '/v2/scrape',
			body: { url: 'https://apotheke.example/hours', formats: ['markdown'], onlyMainContent: true }
		});
	});

	it('says when the page answered with an error', async () => {
		const page = await readWebPage('https://gone.example/');
		expect(page.status).toBe(404);
		expect(page.markdown).toBe('Not found');
	});
});

describe('webUrl', () => {
	it('takes web addresses, with or without https://', () => {
		expect(webUrl(' example.com/a b ')).toBe('https://example.com/a%20b');
		expect(webUrl('http://example.com')).toBe('http://example.com/');
	});

	it('refuses what is not a web page', () => {
		expect(() => webUrl('file:///etc/passwd')).toThrow('Only web pages can be read');
		expect(() => webUrl('http://')).toThrow("isn't a web address");
	});
});
