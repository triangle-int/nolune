import type { Firecrawl } from 'firecrawl';
import { apiKeyHelp, configuredApiKey } from './config.ts';

/*
 * The web for the agent: `nolune web search` and `nolune web read`, through Firecrawl's SDK, loaded
 * on first use. Without a key Firecrawl answers anyway, on its free tier, which allows each IP
 * address so many searches and pages a day; a key (Models & keys, `nolune key set firecrawl`)
 * lifts that to its plan's credits. FIRECRAWL_API_URL points it at a Firecrawl of the family's
 * own. The gateway has no other code for the web: the agent runs these like any other command.
 */

type Env = Readonly<Record<string, string | undefined>>;

export function firecrawlBaseUrl(env: Env = process.env): string {
	return (env.FIRECRAWL_API_URL || process.env.FIRECRAWL_API_URL || 'https://api.firecrawl.dev')
		.trim()
		.replace(/\/+$/, '');
}

/**
 * The key in use: Models & keys', else the gateway's environment's, else the command's own
 * (`nolune env set FIRECRAWL_API_KEY`, from before Firecrawl had a place there). Null: the free tier.
 */
function firecrawlKey(env: Env): string | null {
	return configuredApiKey('firecrawl')?.key ?? (env.FIRECRAWL_API_KEY?.trim() || null);
}

let sdk: typeof import('firecrawl') | undefined;
let cached: { key: string | null; url: string; client: Firecrawl } | undefined;

async function client(key: string | null, url: string): Promise<Firecrawl> {
	const { Firecrawl: Client } = (sdk ??= await import('firecrawl'));
	if (cached?.key !== key || cached.url !== url) {
		// An empty key, not a missing one: the SDK would take FIRECRAWL_API_KEY from the gateway's
		// environment, and it sends no key at all for the free tier.
		cached = { key, url, client: new Client({ apiKey: key ?? '', apiUrl: url }) };
	}
	return cached.client;
}

/** What Firecrawl is given to work on a page or a search, on its side. */
const TIMEOUT_MS = 60_000;

export class WebError extends Error {}

/**
 * In words for the agent and the family. The SDK throws its SdkError with the HTTP status and
 * Firecrawl's message, or the network's code when there was no answer.
 */
function describeFailure(err: unknown, doing: string, keyed: boolean): WebError {
	const e = err as { status?: unknown; code?: unknown; message?: unknown };
	const status = typeof e.status === 'number' ? e.status : null;
	const code = typeof e.code === 'string' ? e.code : null;
	// Its first line: the rest is how to send a key, which isn't the family's to do by hand.
	const said =
		typeof e.message === 'string' ? e.message.trim().split('\n')[0].trim().replace(/\.+$/, '') : '';
	if (status === 401 || status === 403) {
		return new WebError(
			keyed
				? `Firecrawl didn't accept its API key. ${apiKeyHelp('firecrawl')}`
				: `Firecrawl wants an API key${said ? ` (${said})` : ''}. ${apiKeyHelp('firecrawl')}`
		);
	}
	if (status === 402) {
		return new WebError(
			"The Firecrawl key's credits are used up. Add more at firecrawl.dev, or wait for its plan's next month."
		);
	}
	if (status === 429) {
		return new WebError(
			keyed
				? 'Firecrawl is getting too many requests from this key. Try again in a minute.'
				: `Firecrawl's free tier has had enough from this computer for today${said ? ` (${said})` : ''}. ${apiKeyHelp('firecrawl')} A free Firecrawl account comes with more.`
		);
	}
	if (status === 408 || code === 'ETIMEDOUT' || code === 'ECONNABORTED') {
		return new WebError(`Firecrawl didn't finish ${doing} in time. Try again.`);
	}
	if (status === null && code) return new WebError(`Couldn't reach Firecrawl (${code}).`);
	return new WebError(
		`Firecrawl couldn't finish ${doing}${status ? ` (${status})` : ''}${said ? `: ${said}` : ''}.`
	);
}

/** Stops waiting when the command is stopped; the SDK has no way to cancel its request. */
function untilAborted<T>(work: Promise<T>, signal: AbortSignal | undefined): Promise<T> {
	if (!signal) return work;
	return new Promise<T>((resolve, reject) => {
		const abort = () => reject(signal.reason);
		signal.addEventListener('abort', abort, { once: true });
		work.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
	});
}

async function call<T>(
	doing: string,
	options: WebOptions,
	run: (firecrawl: Firecrawl) => Promise<T>
): Promise<T> {
	options.signal?.throwIfAborted();
	const env = options.env ?? process.env;
	const key = firecrawlKey(env);
	const firecrawl = await client(key, firecrawlBaseUrl(env));
	try {
		return await untilAborted(run(firecrawl), options.signal);
	} catch (err) {
		if (options.signal?.aborted) throw err;
		throw describeFailure(err, doing, key !== null);
	}
}

export interface WebOptions {
	/** The command's environment: FIRECRAWL_API_KEY and FIRECRAWL_API_URL may be given there. */
	env?: Env;
	signal?: AbortSignal;
}

/** How far back a search looks, as Google's `tbs` that Firecrawl takes. */
export const WEB_RECENT = { day: 'qdr:d', week: 'qdr:w', month: 'qdr:m', year: 'qdr:y' } as const;
export type WebRecent = keyof typeof WEB_RECENT;

export const DEFAULT_WEB_RESULTS = 5;
export const MAX_WEB_RESULTS = 20;

export interface WebSearchOptions extends WebOptions {
	limit?: number;
	/** News articles, with their dates, instead of web pages. */
	news?: boolean;
	recent?: WebRecent;
	/** ISO 3166-1 alpha-2, like `de`: results as someone there gets them. */
	country?: string;
}

export interface WebResult {
	title: string;
	url: string;
	/** The search's snippet of the page. */
	text: string;
	/** When a news article came out, as the search said it. */
	date: string | null;
}

export async function searchWeb(
	query: string,
	options: WebSearchOptions = {}
): Promise<{ results: WebResult[]; warning: string | null }> {
	const limit = options.limit ?? DEFAULT_WEB_RESULTS;
	const data = await call('the search', options, (firecrawl) =>
		firecrawl.search(query, {
			limit,
			sources: [options.news ? 'news' : 'web'],
			...(options.recent ? { tbs: WEB_RECENT[options.recent] } : {}),
			...(options.country ? { country: options.country.toUpperCase() } : {}),
			timeout: TIMEOUT_MS
		})
	);
	const found = (options.news ? data.news : data.web) ?? [];
	const results = found.flatMap((item): WebResult[] => {
		const r = item as {
			url?: string;
			title?: string;
			description?: string;
			snippet?: string;
			date?: string;
		};
		if (!r.url) return [];
		return [
			{
				title: r.title?.trim() || r.url,
				url: r.url,
				text: (r.description ?? r.snippet ?? '').trim(),
				date: r.date?.trim() || null
			}
		];
	});
	return { results: results.slice(0, limit), warning: data.warning ?? null };
}

/** `example.com/page` as `https://example.com/page`; anything but http and https is refused. */
export function webUrl(text: string): string {
	const trimmed = text.trim();
	const withScheme = /^[a-z][a-z0-9+.-]*:/i.test(trimmed) ? trimmed : `https://${trimmed}`;
	let url: URL;
	try {
		url = new URL(withScheme);
	} catch {
		throw new WebError(`"${trimmed}" isn't a web address.`);
	}
	if (url.protocol !== 'http:' && url.protocol !== 'https:') {
		throw new WebError(`Only web pages can be read (http or https), not ${url.protocol} ones.`);
	}
	return url.href;
}

export interface WebPage {
	/** Where the page ended up, after redirects. */
	url: string;
	title: string | null;
	/** Its main content, without menus and footers. */
	markdown: string;
	/** The page answered with an error status, which its text may explain. */
	status: number | null;
	warning: string | null;
}

/** A page (or a PDF on the web) as Markdown. */
export async function readWebPage(address: string, options: WebOptions = {}): Promise<WebPage> {
	const url = webUrl(address);
	const doc = await call('reading the page', options, (firecrawl) =>
		firecrawl.scrape(url, {
			formats: ['markdown'],
			onlyMainContent: true,
			timeout: TIMEOUT_MS,
			// A big PDF can keep Firecrawl busy for minutes; the command answers in time instead.
			autoResume: false
		})
	);
	const meta = doc.metadata ?? {};
	const status = typeof meta.statusCode === 'number' ? meta.statusCode : null;
	return {
		url: meta.url || meta.sourceURL || url,
		title: meta.title?.trim() || null,
		markdown: doc.markdown?.trim() ?? '',
		status: status !== null && status >= 400 ? status : null,
		warning: doc.warning ?? null
	};
}
