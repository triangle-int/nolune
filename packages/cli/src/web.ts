import { mkdirSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import {
	DEFAULT_WEB_RESULTS,
	MAX_WEB_RESULTS,
	WEB_RECENT,
	readWebPage,
	searchWeb,
	type WebPage,
	type WebRecent
} from '@nolune/core';
import type { Io } from './io.ts';

export const WEB_HELP = `The web (through Firecrawl: its free tier, limited per day, until \`nolune key set firecrawl\`)
  nolune web search <query> [--limit N] [--news] [--recent day|week|month|year] [--country CC]
                                             the top results' titles, addresses and snippets
                                             (${DEFAULT_WEB_RESULTS} unless --limit, at most ${MAX_WEB_RESULTS}); --news for news
                                             articles, --country as someone there sees them (de)
  nolune web read <url> [--out FILE]         a page's main text (or a PDF's) as Markdown; a long
                                             one is saved whole to a file, whose path it prints`;

/** More than this and the page goes to a file: a command's output is cut at 30 KB. */
const MAX_PRINTED = 20_000;

function expandHome(path: string): string {
	if (path === '~') return homedir();
	return path.startsWith('~/') ? join(homedir(), path.slice(2)) : path;
}

async function search(io: Io, args: string[]): Promise<void> {
	const { values, positionals } = parseArgs({
		args,
		allowPositionals: true,
		options: {
			limit: { type: 'string', short: 'n' },
			news: { type: 'boolean' },
			recent: { type: 'string' },
			country: { type: 'string' }
		}
	});
	const query = positionals.join(' ').trim();
	if (!query) throw new Error('say what to look for: nolune web search "<query>"');
	const limit = values.limit === undefined ? DEFAULT_WEB_RESULTS : Number(values.limit);
	if (!Number.isInteger(limit) || limit < 1 || limit > MAX_WEB_RESULTS) {
		throw new Error(`--limit is between 1 and ${MAX_WEB_RESULTS}.`);
	}
	const recent = values.recent?.trim().toLowerCase();
	if (recent !== undefined && !Object.hasOwn(WEB_RECENT, recent)) {
		throw new Error(`--recent is one of ${Object.keys(WEB_RECENT).join(', ')}.`);
	}
	const country = values.country?.trim();
	if (country !== undefined && !/^[a-z]{2}$/i.test(country)) {
		throw new Error('--country is a two-letter country code, like de or us.');
	}

	const { results, warning } = await searchWeb(query, {
		limit,
		news: values.news,
		recent: recent as WebRecent | undefined,
		country,
		env: io.env,
		signal: io.signal
	});
	if (warning) io.log(`Firecrawl: ${warning}`);
	if (!results.length) {
		io.log(`No results for "${query}".`);
		return;
	}
	io.log(
		results
			.map((r, i) => {
				const text = [r.text, r.date && `(${r.date})`].filter(Boolean).join(' ');
				return `${i + 1}. ${r.title}\n   ${r.url}${text ? `\n   ${text}` : ''}`;
			})
			.join('\n\n')
	);
}

/** A name for the page's file: when it was read, and its address in a few words. */
function fileName(url: string, now: Date): string {
	const p = (n: number) => String(n).padStart(2, '0');
	const stamp = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
	const { hostname, pathname } = new URL(url);
	const words = `${hostname.replace(/^www\./, '')} ${pathname}`
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 60)
		.replace(/-+$/, '');
	return `${stamp}-${words || 'page'}.md`;
}

function pageText(page: WebPage): string {
	return `${page.title ? `# ${page.title}\n` : ''}${page.url}\n\n${page.markdown}\n`;
}

async function read(io: Io, args: string[]): Promise<void> {
	const { values, positionals } = parseArgs({
		args,
		allowPositionals: true,
		options: { out: { type: 'string', short: 'o' } }
	});
	if (positionals.length !== 1) throw new Error('usage: nolune web read <url> [--out FILE]');
	const page = await readWebPage(positionals[0], { env: io.env, signal: io.signal });
	if (page.warning) io.log(`Firecrawl: ${page.warning}`);
	if (page.status) io.log(`The page answered ${page.status}; this is what it showed.`);
	if (!page.markdown) {
		io.log(`${page.url} has no text Firecrawl could read.`);
		return;
	}

	const text = pageText(page);
	if (values.out) {
		const file = resolve(io.cwd, expandHome(values.out));
		mkdirSync(dirname(file), { recursive: true });
		writeFileSync(file, text);
		io.log(`Saved ${page.url} (${text.length} characters) to ${file}`);
		return;
	}
	if (text.length <= MAX_PRINTED) {
		io.stdout(text);
		return;
	}
	const dir = join(io.env.TMPDIR || tmpdir(), 'nolune-web');
	const file = join(dir, fileName(page.url, new Date()));
	mkdirSync(dir, { recursive: true });
	writeFileSync(file, text);
	const cut = text.lastIndexOf('\n', MAX_PRINTED);
	const shown = text.slice(0, cut > MAX_PRINTED / 2 ? cut : MAX_PRINTED);
	io.log(
		`${shown}\n\n[${text.length - shown.length} more characters. The whole page is in ${file}: grep it, or read on with sed -n.]`
	);
}

export async function webCommand(io: Io, action: string | undefined, args: string[]) {
	if (action === 'search') return search(io, args);
	if (action === 'read') return read(io, args);
	throw new Error('usage: nolune web search <query> | nolune web read <url>. See `nolune help`.');
}
