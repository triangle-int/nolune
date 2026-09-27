import type { OpenAI } from 'openai';
import {
	CODEX_SIGN_IN_HELP,
	CodexAuthError,
	codexCredentials,
	type CodexCredentials
} from './codex-auth.ts';
import {
	isSdkError,
	loadSdk,
	openTurn,
	readStream,
	supportsReasoning,
	toResponsesInput,
	turnRequest,
	type TurnOptions
} from './openai-chat.ts';

/*
 * Chats on a ChatGPT plan: the `codex` provider. ChatGPT's Codex backend serves the same
 * Responses API as OpenAI's platform, so requests are built and read by openai-chat.ts; this
 * module signs them with the ChatGPT sign-in (codex-auth.ts) and sends them where Codex does.
 * The rest of btw calls it through models.ts.
 *
 * What the backend does differently from OpenAI's API:
 * - every request streams, and none takes `max_output_tokens`;
 * - there's no Files API, so pictures go inline and PDFs as their path (attachments.ts);
 * - the models are the plan's, listed in Codex's catalog with their context windows;
 * - use counts against the plan's Codex limits, which reset every few hours and weekly.
 */

/** BTW_CODEX_BASE_URL points it elsewhere, for tests. */
function baseUrl(): string {
	return (process.env.BTW_CODEX_BASE_URL || 'https://chatgpt.com/backend-api/codex').replace(
		/\/+$/,
		''
	);
}

/** A problem with what the backend said, in words for people. */
class CodexError extends Error {}

/** The catalog lists the models a Codex client of this version can use. */
const CLIENT_VERSION = '0.157.1';
const REQUEST_TIMEOUT_MS = 60_000;

let cached: { key: string; client: OpenAI } | undefined;

async function clientFor(credentials: CodexCredentials): Promise<OpenAI> {
	const baseURL = baseUrl();
	const key = [baseURL, credentials.accountId, credentials.accessToken].join('\n');
	if (cached?.key !== key) {
		const { OpenAI: Client } = await loadSdk();
		const client = new Client({
			apiKey: credentials.accessToken,
			baseURL,
			// The environment's are for OpenAI's platform, not a ChatGPT account.
			organization: null,
			project: null,
			defaultHeaders: {
				originator: 'btw',
				...(credentials.accountId ? { 'ChatGPT-Account-Id': credentials.accountId } : {})
			}
		});
		cached = { key, client };
	}
	return cached.client;
}

/**
 * Runs `call` with the signed-in account's client. When the backend turns the token down (it
 * ran out early, or was revoked), the sign-in is renewed and `call` runs once more.
 */
async function withClient<T>(
	call: (client: OpenAI, accountId: string | null) => Promise<T>
): Promise<T> {
	const credentials = await codexCredentials();
	try {
		return await call(await clientFor(credentials), credentials.accountId);
	} catch (err) {
		if (!isSdkError(err, 'AuthenticationError')) throw err;
		const renewed = await codexCredentials(credentials.accessToken);
		return call(await clientFor(renewed), renewed.accountId);
	}
}

/** One model call, streamed. See models.ts for what stays fixed between calls. */
export async function streamResponse(opts: TurnOptions): Promise<OpenAI.Responses.Response> {
	const stream = await withClient((client, accountId) =>
		openTurn(`codex:${accountId}`, (summaries) =>
			client.responses.create(turnRequest(opts, summaries), {
				signal: opts.signal,
				// As Codex sends it, next to prompt_cache_key.
				headers: { 'session-id': opts.cacheKey }
			})
		)
	);
	return readStream(stream, opts.onEvent);
}

/**
 * One short exchange at low effort (see models.ts). Streamed, since the backend only streams,
 * and without `maxTokens`, which it doesn't take.
 */
export async function createResponse(opts: {
	model: string;
	system: string;
	input: string;
	timeoutMs: number;
}): Promise<OpenAI.Responses.Response> {
	const signal = AbortSignal.timeout(opts.timeoutMs);
	const stream = await withClient((client) =>
		client.responses.create(
			{
				model: opts.model,
				instructions: opts.system,
				input: toResponsesInput([{ role: 'user', content: opts.input }]),
				store: false,
				stream: true,
				...(supportsReasoning(opts.model) ? { reasoning: { effort: 'low' as const } } : {})
			},
			{ signal }
		)
	);
	return readStream(stream, () => {});
}

// --- models ---

/** A model in Codex's catalog, as btw needs it. */
export interface CodexModel {
	id: string;
	name: string;
	contextWindow: number | null;
	/** Hidden models work but aren't offered in Codex's own model picker. */
	listed: boolean;
}

/** The models the signed-in plan can use, as Codex's catalog lists them. */
export async function listCodexModels(): Promise<CodexModel[]> {
	const body = await withClient((client) =>
		client.get<{ models?: unknown }>('/models', {
			query: { client_version: CLIENT_VERSION },
			timeout: REQUEST_TIMEOUT_MS
		})
	);
	const models = Array.isArray(body.models) ? (body.models as Record<string, unknown>[]) : [];
	return models.flatMap((m) => {
		if (typeof m?.slug !== 'string' || !m.slug) return [];
		const window = m.context_window;
		return [
			{
				id: m.slug,
				name: typeof m.display_name === 'string' ? m.display_name : m.slug,
				contextWindow: typeof window === 'number' && window > 0 ? window : null,
				listed: m.visibility !== 'hide'
			}
		];
	});
}

/** Throws if the plan has no such model; its window otherwise, as the catalog gives it. */
export async function fetchContextWindow(model: string): Promise<number | null> {
	const models = await listCodexModels();
	const found = models.find((m) => m.id === model);
	if (found) return found.contextWindow;
	const offered = models.filter((m) => m.listed).map((m) => m.id);
	throw new CodexError(
		`ChatGPT has no model "${model}" for Codex${offered.length ? `. It has ${offered.join(', ')}` : ''}.`
	);
}

// --- errors ---

/** How long until a limit resets, roughly. */
function resetsIn(resetsAt: unknown): string {
	if (typeof resetsAt !== 'number') return '';
	const minutes = Math.max(1, Math.round((resetsAt * 1000 - Date.now()) / 60_000));
	if (minutes < 90) return ` It resets in ${minutes} minute${minutes === 1 ? '' : 's'}.`;
	const hours = Math.round(minutes / 60);
	if (hours < 48) return ` It resets in about ${hours} hours.`;
	return ` It resets in about ${Math.round(hours / 24)} days.`;
}

export function describeApiError(err: unknown): string {
	if (err instanceof CodexAuthError || err instanceof CodexError) return err.message;
	if (isSdkError(err, 'AuthenticationError')) {
		return `ChatGPT didn't accept btw's sign-in. ${CODEX_SIGN_IN_HELP}`;
	}
	if (isSdkError(err, 'RateLimitError')) {
		const body = err.error as { type?: unknown; resets_at?: unknown } | undefined;
		if (body?.type === 'usage_limit_reached') {
			return `The ChatGPT plan's Codex limit is used up for now.${resetsIn(body.resets_at)}`;
		}
		if (body?.type === 'usage_not_included') return "This ChatGPT plan doesn't include Codex.";
		return 'Rate limited by ChatGPT. Try again shortly.';
	}
	if (isSdkError(err, 'NotFoundError')) return `Model not found: ${shortApiError(err)}`;
	if (isSdkError(err, 'APIConnectionTimeoutError')) return "ChatGPT didn't answer in time.";
	if (isSdkError(err, 'APIConnectionError')) {
		// fetch says "fetch failed"; the reason (ECONNREFUSED, ENOTFOUND...) is in its cause.
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach ChatGPT (${why}).`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `ChatGPT error ${err.status}: ${shortApiError(err)}`;
	}
	return `ChatGPT: ${shortApiError(err)}`;
}

/** The backend's own message, without the status and JSON around it: for notes to the model. */
export function shortApiError(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		// OpenAI's `{error: {message}}`, or `{detail}` from the backend's own checks.
		const body = err.error as { message?: unknown; detail?: unknown } | undefined;
		if (typeof body?.message === 'string') return body.message;
		if (typeof body?.detail === 'string') return body.detail;
	}
	return err instanceof Error ? err.message : String(err);
}
