import { API_KEYS, configuredApiKey, updateConfig, type ApiKeyProvider } from './config.ts';
import { openaiBaseUrl } from './openai.ts';
import { openrouterBaseUrl } from './openrouter.ts';

/*
 * API keys as the admin page and `btw key` handle them. A key never goes back out: the status
 * says where the key in use comes from and shows its last four characters. Saving checks the key
 * with its provider first, and since every request reads config.json, it applies at once.
 */

export interface ApiKeyStatus {
	provider: ApiKeyProvider;
	label: string;
	/** What btw uses it for. */
	purpose: string;
	/** What stops working without it. */
	withoutIt: string;
	/** Where to make one. */
	consoleUrl: string;
	/** Where the key in use comes from, or null without one. */
	source: 'config' | 'env' | null;
	/** The last four characters of the key in use, to tell keys apart. */
	hint: string | null;
	/** The environment variable used when config.json has no key, and whether it's set. */
	env: string;
	envSet: boolean;
}

const ABOUT: Record<ApiKeyProvider, Pick<ApiKeyStatus, 'purpose' | 'withoutIt' | 'consoleUrl'>> = {
	anthropic: {
		purpose: 'Runs chats and automations on Claude models.',
		withoutIt: 'Chats and automations on Claude models stop working until a new key is added.',
		consoleUrl: 'https://console.anthropic.com/settings/keys'
	},
	openai: {
		purpose:
			'Runs chats and automations on OpenAI models, and makes pictures for the Images page and when the agent draws.',
		withoutIt:
			"Chats and automations on OpenAI models stop working, and btw can't make pictures, until a new key is added.",
		consoleUrl: 'https://platform.openai.com/api-keys'
	},
	openrouter: {
		purpose:
			'Runs chats and automations on the models OpenRouter serves (Claude, GPT, Gemini, DeepSeek and many more), with one key and its credits.',
		withoutIt: 'Chats and automations on OpenRouter models stop working until a new key is added.',
		consoleUrl: 'https://openrouter.ai/settings/keys'
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
 * anyone, so it's asked about the key itself.
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
	if (res.ok) return null;

	let message = '';
	try {
		const body = (await res.json()) as { error?: { message?: unknown } };
		if (typeof body.error?.message === 'string') message = body.error.message.trim();
	} catch {
		// not JSON
	}
	// OpenAI's restricted keys may not be allowed to list models while still being allowed to
	// make pictures, and a 429 means the key is real but the account is out of credit or busy.
	if ((res.status === 403 && provider === 'openai') || res.status === 429) {
		return `${label} took the key but said: ${message || res.statusText}`;
	}
	if (res.status === 401 || res.status === 403) {
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
