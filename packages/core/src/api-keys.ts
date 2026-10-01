import { API_KEYS, configuredApiKey, updateConfig, type ApiKeyProvider } from './config.ts';
import { openaiBaseUrl } from './openai.ts';
import { openrouterBaseUrl } from './openrouter.ts';
import { firecrawlBaseUrl } from './web.ts';
import { xaiBaseUrl } from './xai.ts';

/*
 * API keys as the admin page and `nolune key` handle them. A key never goes back out: the status
 * says where the key in use comes from and shows its last four characters. Saving checks the key
 * with its provider first, and since every request reads config.json, it applies at once.
 */

export interface ApiKeyStatus {
	provider: ApiKeyProvider;
	label: string;
	/** What nolune uses it for. */
	purpose: string;
	/** What stops working without it. */
	withoutIt: string;
	/** Where to make one. */
	consoleUrl: string;
	/** What it's for works without it too, within the provider's free allowance (Firecrawl's). */
	optional: boolean;
	/** Where the key in use comes from, or null without one. */
	source: 'config' | 'env' | null;
	/** The last four characters of the key in use, to tell keys apart. */
	hint: string | null;
	/** The environment variable used when config.json has no key, and whether it's set. */
	env: string;
	envSet: boolean;
}

const ABOUT: Record<
	ApiKeyProvider,
	Pick<ApiKeyStatus, 'purpose' | 'withoutIt' | 'consoleUrl' | 'optional'>
> = {
	anthropic: {
		purpose: 'Runs chats and automations on Claude models.',
		withoutIt: 'Chats and automations on Claude models stop working until a new key is added.',
		consoleUrl: 'https://console.anthropic.com/settings/keys',
		optional: false
	},
	openai: {
		purpose:
			'Runs chats and automations on OpenAI models, and makes pictures for the Images page and when the agent draws.',
		withoutIt:
			"Chats and automations on OpenAI models stop working, and nolune can't make pictures, until a new key is added.",
		consoleUrl: 'https://platform.openai.com/api-keys',
		optional: false
	},
	openrouter: {
		purpose:
			'Runs chats and automations on the models OpenRouter serves (Claude, GPT, Gemini, DeepSeek and many more), with one key and its credits.',
		withoutIt: 'Chats and automations on OpenRouter models stop working until a new key is added.',
		consoleUrl: 'https://openrouter.ai/settings/keys',
		optional: false
	},
	xai: {
		purpose: "Runs chats and automations on xAI's Grok models.",
		withoutIt: 'Chats and automations on Grok models stop working until a new key is added.',
		consoleUrl: 'https://console.x.ai',
		optional: false
	},
	firecrawl: {
		purpose:
			"Searches the web and reads pages for the agent. Without a key it uses Firecrawl's free tier, which allows this computer so many searches a day.",
		withoutIt: "Web searches go back to Firecrawl's free tier, with its daily limit.",
		consoleUrl: 'https://www.firecrawl.dev/app/api-keys',
		optional: true
	}
};

export function apiKeyStatuses(): ApiKeyStatus[] {
	return (Object.keys(API_KEYS) as ApiKeyProvider[]).map((provider) => {
		const { label, env } = API_KEYS[provider];
		const found = configuredApiKey(provider);
		return {
			provider,
			label,
			...ABOUT[provider],
			source: found?.source ?? null,
			hint: found && found.key.length >= 16 ? found.key.slice(-4) : null,
			env,
			envSet: !!process.env[env]
		};
	});
}

export class ApiKeyError extends Error {
	/** `invalid`: it can't be a key. `rejected`: the provider said no. `unchecked`: it couldn't be asked. */
	readonly reason: 'invalid' | 'rejected' | 'unchecked';
	constructor(message: string, reason: ApiKeyError['reason']) {
		super(message);
		this.reason = reason;
	}
}

/** The key as pasted, without surrounding space. Throws if it can't be one. */
export function normalizeApiKey(value: string): string {
	const key = value.trim();
	if (!key) throw new ApiKeyError('Paste the key first.', 'invalid');
	if (/\s/.test(key)) {
		throw new ApiKeyError("That doesn't look like an API key: it has spaces in it.", 'invalid');
	}
	if (key.length > 512) throw new ApiKeyError("That's too long for an API key.", 'invalid');
	return key;
}

const CHECK_TIMEOUT_MS = 20_000;

/**
 * Listing models is free and needs nothing but a valid key. OpenRouter lists its models for
 * anyone, so it's asked about the key itself, and so is xAI, which also says whether the key or
 * its team is blocked. Firecrawl has no models, so it's asked for the key's credits.
 */
function checkRequest(
	provider: ApiKeyProvider,
	key: string
): { url: string; headers: Record<string, string> } {
	if (provider === 'anthropic') {
		const base = (process.env.ANTHROPIC_BASE_URL || 'https://api.anthropic.com').replace(
			/\/+$/,
			''
		);
		return {
			url: `${base}/v1/models?limit=1`,
			headers: { 'x-api-key': key, 'anthropic-version': '2023-06-01' }
		};
	}
	if (provider === 'openrouter') {
		return { url: `${openrouterBaseUrl()}/key`, headers: { authorization: `Bearer ${key}` } };
	}
	if (provider === 'xai') {
		return { url: `${xaiBaseUrl()}/api-key`, headers: { authorization: `Bearer ${key}` } };
	}
	if (provider === 'firecrawl') {
		return {
			url: `${firecrawlBaseUrl()}/v2/team/credit-usage`,
			headers: { authorization: `Bearer ${key}` }
		};
	}
	return { url: `${openaiBaseUrl()}/models`, headers: { authorization: `Bearer ${key}` } };
}

/** fetch says "fetch failed"; the reason is in its cause (ECONNREFUSED, ENOTFOUND...). */
function networkError(err: unknown): string {
	if (!(err instanceof Error)) return String(err);
	if (err.name === 'TimeoutError') return 'no answer';
	const cause = err.cause as { code?: unknown; message?: unknown } | undefined;
	if (typeof cause?.code === 'string') return cause.code;
	if (typeof cause?.message === 'string') return cause.message;
	return err.message;
}

/**
 * Asks the provider whether it takes the key. Resolves with a warning when the key works but the
 * account has a problem (out of credit, rate limited); throws an ApiKeyError otherwise.
 */
export async function checkApiKey(provider: ApiKeyProvider, key: string): Promise<string | null> {
	const { label } = API_KEYS[provider];
	const { url, headers } = checkRequest(provider, key);
	let res: Response;
	try {
		res = await fetch(url, { headers, signal: AbortSignal.timeout(CHECK_TIMEOUT_MS) });
	} catch (err) {
		throw new ApiKeyError(
			`Couldn't reach ${label} to check the key (${networkError(err)}).`,
			'unchecked'
		);
	}
	if (res.ok) return provider === 'xai' ? xaiKeyWarning(res) : null;

	let message = '';
	try {
		// xAI's errors are `{ code, error }`, its message a string.
		const body = (await res.json()) as { error?: { message?: unknown } | string };
		const found = typeof body.error === 'string' ? body.error : body.error?.message;
		if (typeof found === 'string') message = found.trim();
	} catch {
		// not JSON
	}
	// OpenAI's restricted keys may not be allowed to list models while still being allowed to
	// make pictures, and a 429 means the key is real but the account is out of credit or busy.
	if ((res.status === 403 && provider === 'openai') || res.status === 429) {
		return `${label} took the key but said: ${message || res.statusText}`;
	}
	// xAI answers a key it doesn't know with a 400.
	if (
		res.status === 401 ||
		res.status === 403 ||
		(provider === 'xai' && res.status === 400 && /api key/i.test(message))
	) {
		throw new ApiKeyError(
			`${label} didn't accept this key. Check that it was copied whole and hasn't been deleted.`,
			'rejected'
		);
	}
	throw new ApiKeyError(
		`${label} couldn't check the key (${res.status}${message ? `: ${message}` : ''}). Try again in a moment.`,
		'unchecked'
	);
}

/** What xAI says about a key it knows: whether it, or its team, can't make requests. */
async function xaiKeyWarning(res: Response): Promise<string | null> {
	let body: { team_blocked?: unknown; api_key_blocked?: unknown; api_key_disabled?: unknown };
	try {
		body = (await res.json()) as typeof body;
	} catch {
		return null;
	}
	if (body.api_key_disabled === true) return 'xAI took the key, but it is disabled.';
	if (body.api_key_blocked === true) return 'xAI took the key, but it is blocked.';
	if (body.team_blocked === true) {
		return 'xAI took the key, but its team is blocked: it may be out of credits or over its spending limit (see https://console.x.ai).';
	}
	return null;
}

/** Saves the key to config.json, where it takes precedence over the environment's. */
export function saveApiKey(provider: ApiKeyProvider, key: string): void {
	const { field } = API_KEYS[provider];
	updateConfig((config) => {
		config[field] = key;
	});
}

/** Removes the saved key. The environment's, if there is one, is used from then on. */
export function removeApiKey(provider: ApiKeyProvider): void {
	const { field } = API_KEYS[provider];
	updateConfig((config) => {
		delete config[field];
	});
}
