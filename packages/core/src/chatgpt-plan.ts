import type { OpenAI } from 'openai';
import {
	CHATGPT_SIGN_IN_HELP,
	chatGptCredentials,
	type ChatGptCredentials
} from './chatgpt-sign-in.ts';
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
import { PlanError } from './plans.ts';

/*
 * Chats on the ChatGPT plan: a Plus, Pro or Business plan someone signed in to with ChatGPT
 * (chatgpt-sign-in.ts), used the way OpenAI's Codex uses it. Like the Claude plan (claude-plan.ts)
 * it runs on a subscription instead of an API key, and what people see of the two is shared
 * (plans.ts). Unlike it, btw holds the sign-in and runs its own agent loop: ChatGPT's Codex
 * backend serves the same Responses API as OpenAI's platform, so requests are built and read by
 * openai-chat.ts and this module only signs them and sends them where Codex does. The rest of
 * btw calls it through models.ts. Its failures come out as PlanErrors, like the Claude plan's.
 *
 * What the backend does differently from OpenAI's API:
 * - every request streams, and none takes `max_output_tokens`;
 * - there's no Files API, so pictures go inline and PDFs as their path (attachments.ts);
 * - the models are the plan's, listed in Codex's catalog with their context windows;
 * - use counts against the plan's Codex limits, which reset every few hours and weekly.
 */

/** BTW_CHATGPT_BASE_URL points it elsewhere, for tests. */
function baseUrl(): string {
	return (process.env.BTW_CHATGPT_BASE_URL || 'https://chatgpt.com/backend-api/codex').replace(
		/\/+$/,
		''
	);
}

/** The catalog lists the models a Codex client of this version can use. */
const CLIENT_VERSION = '0.157.1';
const REQUEST_TIMEOUT_MS = 60_000;

let cached: { key: string; client: OpenAI } | undefined;

async function clientFor(credentials: ChatGptCredentials): Promise<OpenAI> {
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
	const credentials = await chatGptCredentials();
	try {
		return await call(await clientFor(credentials), credentials.accountId);
	} catch (err) {
		if (!isSdkError(err, 'AuthenticationError')) throw err;
		const renewed = await chatGptCredentials(credentials.accessToken);
		return call(await clientFor(renewed), renewed.accountId);
	}
}

/** One model call, streamed. See models.ts for what stays fixed between calls. */
export function streamResponse(opts: TurnOptions): Promise<OpenAI.Responses.Response> {
	return failing(async () => {
		const stream = await withClient((client, accountId) =>
			openTurn(`chatgpt:${accountId}`, (summaries) =>
				client.responses.create(turnRequest(opts, summaries), {
					signal: opts.signal,
					// As Codex sends it, next to prompt_cache_key.
					headers: { 'session-id': opts.cacheKey }
				})
			)
		);
		return readStream(stream, opts.onEvent);
	});
}

/**
 * One short exchange at low effort (see models.ts). Streamed, since the backend only streams,
 * and without `maxTokens`, which it doesn't take.
 */
export function createResponse(opts: {
	model: string;
	system: string;
	input: string;
	timeoutMs: number;
}): Promise<OpenAI.Responses.Response> {
	return failing(async () => {
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
	});
}

// --- models ---

/** A model in Codex's catalog, as btw needs it. */
export interface ChatGptModel {
	id: string;
	name: string;
	contextWindow: number | null;
	/** Hidden models work but aren't offered in Codex's own model picker. */
	listed: boolean;
}

/** The models the signed-in plan can use, as Codex's catalog lists them. */
export async function listChatGptModels(): Promise<ChatGptModel[]> {
	const body = await failing(() =>
		withClient((client) =>
			client.get<{ models?: unknown }>('/models', {
				query: { client_version: CLIENT_VERSION },
				timeout: REQUEST_TIMEOUT_MS
			})
		)
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
	const models = await listChatGptModels();
	const found = models.find((m) => m.id === model);
	if (found) return found.contextWindow;
	const offered = models.filter((m) => m.listed).map((m) => m.id);
	throw new PlanError(
		`ChatGPT has no model "${model}" for Codex${offered.length ? `. It has ${offered.join(', ')}` : ''}.`,
		'not_found'
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

/**
 * Runs `work`, turning what fails into a PlanError in words for people, as the Claude plan does.
 * A stop stays what it is, so the runner can tell it apart.
 */
async function failing<T>(work: () => Promise<T>): Promise<T> {
	try {
		return await work();
	} catch (err) {
		if (err instanceof PlanError || isSdkError(err, 'APIUserAbortError')) throw err;
		const kind = isSdkError(err, 'APIError') ? (err.type ?? err.code ?? null) : null;
		throw new PlanError(describe(err), kind, { cause: err });
	}
}

function describe(err: unknown): string {
	if (isSdkError(err, 'AuthenticationError')) {
		return `ChatGPT didn't accept btw's sign-in. ${CHATGPT_SIGN_IN_HELP}`;
	}
	if (isSdkError(err, 'RateLimitError')) {
		const body = err.error as { type?: unknown; resets_at?: unknown } | undefined;
		if (body?.type === 'usage_limit_reached') {
			return `The ChatGPT plan's Codex limit is used up for now.${resetsIn(body.resets_at)}`;
		}
		if (body?.type === 'usage_not_included') return "This ChatGPT plan doesn't include Codex.";
		return 'Rate limited by ChatGPT. Try again shortly.';
	}
	if (isSdkError(err, 'NotFoundError')) return `Model not found: ${backendMessage(err)}`;
	if (isSdkError(err, 'APIConnectionTimeoutError')) return "ChatGPT didn't answer in time.";
	if (isSdkError(err, 'APIConnectionError')) {
		// fetch says "fetch failed"; the reason (ECONNREFUSED, ENOTFOUND...) is in its cause.
		const cause = err.cause as
			{ code?: string; cause?: { code?: string; message?: string } } | undefined;
		const why = cause?.code ?? cause?.cause?.code ?? cause?.cause?.message ?? err.message;
		return `Couldn't reach ChatGPT (${why}).`;
	}
	if (isSdkError(err, 'APIError') && err.status) {
		return `ChatGPT error ${err.status}: ${backendMessage(err)}`;
	}
	return `ChatGPT: ${backendMessage(err)}`;
}

/** The backend's own message, without the status and JSON around it. */
function backendMessage(err: unknown): string {
	if (isSdkError(err, 'APIError')) {
		// OpenAI's `{error: {message}}`, or `{detail}` from the backend's own checks.
		const body = err.error as { message?: unknown; detail?: unknown } | undefined;
		if (typeof body?.message === 'string') return body.message;
		if (typeof body?.detail === 'string') return body.detail;
	}
	return err instanceof Error ? err.message : String(err);
}
