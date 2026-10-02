import { readAccount, updateAccount } from './accounts.ts';
import type { Db } from './db.ts';
import {
	admit,
	charge,
	hasPlan,
	micros,
	usage,
	type Account,
	type Admission,
	type Request as PlanRequest
} from './limits.ts';
import {
	chatModels,
	costOf,
	EMBEDDING_MODELS,
	isFree,
	type ImageModel,
	type Model,
	type OpenRouter
} from './openrouter.ts';
import { RateLimit } from './rate-limit.ts';
import { tapStream } from './stream.ts';

/*
 * nolune's API in front of OpenRouter (DESIGN.md, The nolune plan). A request is checked against
 * the plan's limits before it goes, passed on as it came with nolune's key (and `max_tokens`, when
 * it asks for more or none), and charged what OpenRouter says it cost once it's done: from the
 * reply, a stream's last chunk, or for a stream cut off before then, OpenRouter's record of the
 * generation, asked a few times as it comes in. Pictures go to OpenRouter's Image API the same way.
 */

/** The most a chat request may write: set when it asks for more, or says nothing. */
export const MAX_TOKENS = 32_000;
/** Requests one account may have going at once; and start in a minute (below). */
const AT_ONCE = 8;
/** When to ask again what a cut-off stream cost: OpenRouter has it within a minute or so. */
const SETTLE_AFTER = [5_000, 20_000, 60_000, 180_000];
/** Pictures one request may ask for: each costs cents, and more at once is almost always a slip. */
export const MAX_IMAGES = 4;

export type Endpoint = 'chat' | 'embedding' | 'image';

const PATHS = { chat: '/chat/completions', embedding: '/embeddings', image: '/images' } as const;

export interface ProxyOptions {
	db: Db;
	openrouter: OpenRouter;
	now?: () => number;
	/** Waits before each ask of what a cut-off stream cost. */
	settleAfter?: number[];
	log?: Pick<Console, 'warn' | 'error'>;
}

/** An error in OpenAI's shape, which the gateway's SDK reads. */
export function apiError(
	status: number,
	code: string,
	message: string,
	headers: Record<string, string> = {},
	extra: Record<string, unknown> = {}
): Response {
	return Response.json({ error: { code, message, ...extra } }, { status, headers });
}

export function notSignedIn(): Response {
	return apiError(401, 'not_signed_in', 'Link nolune to the plan again: its sign-in has ended.');
}

const REFUSALS: Record<Extract<Admission, { ok: false }>['code'], string> = {
	five_hour_limit: "nolune's 5-hour limit is reached.",
	weekly_limit: "nolune's weekly limit is reached.",
	background_share:
		"Background work has used its share of nolune's limits; the rest is kept for people.",
	credits_spent: "The nolune plan's credits are spent."
};

/** Every response says where the limits stand (`x-nolune-usage`), for the gateway to show. */
function usageHeader(account: Account | null, now: number): Record<string, string> {
	return account ? { 'x-nolune-usage': JSON.stringify(usage(account, now)) } : {};
}

/** What a request's input will cost at least: its text, at the price of reading it from a cache. */
function inputCost(body: string, model: Model | undefined): number | undefined {
	const price = Number(model?.pricing?.input_cache_read ?? model?.pricing?.prompt);
	if (!Number.isFinite(price) || price <= 0) return undefined;
	// Pictures and PDFs go as data URLs, whose length says little about their tokens.
	const text = body.replace(/data:[\w.+-]+\/[\w.+-]+;base64,[A-Za-z0-9+/=]+/g, '');
	return micros((text.length / 4) * price);
}

/** The body to send: as it came, unless it asks to write more than MAX_TOKENS, or doesn't say. */
function capped(body: string, json: Record<string, unknown>): string {
	const field = 'max_completion_tokens' in json ? 'max_completion_tokens' : 'max_tokens';
	const asked = json[field];
	if (typeof asked === 'number' && asked > 0 && asked <= MAX_TOKENS) return body;
	return JSON.stringify({ ...json, [field]: MAX_TOKENS });
}

export class Proxy {
	private readonly db: Db;
	private readonly openrouter: OpenRouter;
	private readonly now: () => number;
	private readonly settleAfter: number[];
	private readonly log: Pick<Console, 'warn' | 'error'>;
	private readonly going = new Map<string, number>();
	private readonly perMinute = new RateLimit(120, 60_000);
	private readonly settling = new Set<Promise<unknown>>();

	constructor(options: ProxyOptions) {
		this.db = options.db;
		this.openrouter = options.openrouter;
		this.now = options.now ?? Date.now;
		this.settleAfter = options.settleAfter ?? SETTLE_AFTER;
		this.log = options.log ?? console;
	}

	/** Until every charge still being worked out is made (for tests). */
	async settled(): Promise<void> {
		while (this.settling.size) await Promise.all([...this.settling]);
	}

	private track(work: Promise<unknown>) {
		const tracked = work
			.catch((err) => this.log.error('[nolune api] charging a request failed:', err))
			.finally(() => this.settling.delete(tracked));
		this.settling.add(tracked);
	}

	async handle(input: {
		userId: string;
		endpoint: Endpoint;
		headers: Headers;
		body: string;
		signal?: AbortSignal;
	}): Promise<Response> {
		const { userId, endpoint, headers, body, signal } = input;
		const now = this.now();

		let json: Record<string, unknown>;
		try {
			json = JSON.parse(body);
			if (!json || typeof json !== 'object' || Array.isArray(json)) throw new Error();
		} catch {
			return apiError(400, 'invalid_request', "The request's body isn't a JSON object.");
		}
		const modelId = typeof json.model === 'string' ? json.model : '';
		let model: Model | undefined;
		if (endpoint === 'image') {
			const refused = await this.imageRequestProblem(json, modelId);
			if (refused) return refused;
		} else if (endpoint === 'chat') {
			// A model as listed, or a variant (`:nitro`) of one, but never the free one (chatModels).
			const offered = chatModels(await this.openrouter.models());
			const base = modelId.split(':')[0];
			model = isFree(modelId)
				? undefined
				: (offered.find((m) => m.id === modelId) ?? offered.find((m) => m.id === base));
			if (!model) {
				return apiError(
					400,
					'model_not_offered',
					`The nolune plan doesn't offer ${modelId || 'that model'}.`
				);
			}
		} else if (!EMBEDDING_MODELS.includes(modelId)) {
			return apiError(
				400,
				'model_not_offered',
				`The nolune plan doesn't offer ${modelId || 'that model'} for embeddings.`
			);
		}

		if (!this.perMinute.allow(userId, now) || (this.going.get(userId) ?? 0) >= AT_ONCE) {
			return apiError(
				429,
				'too_many_requests',
				'Too many requests at once. Try again in a moment.',
				{
					'retry-after': '5'
				}
			);
		}

		const request: PlanRequest = {
			kind: endpoint,
			use: headers.get('x-nolune-use') === 'background' ? 'background' : 'person',
			continuing: headers.get('x-nolune-turn') === 'continue',
			inputCost: endpoint === 'chat' ? inputCost(body, model) : undefined
		};
		const account = await readAccount(this.db, userId);
		if (!hasPlan(account)) return apiError(402, 'no_plan', 'This account has no nolune plan.');
		const admission = admit(account, request, now);
		if (!admission.ok) {
			const resetsAt = admission.resetsAt === null ? null : new Date(admission.resetsAt);
			const wait = resetsAt ? Math.max(1, Math.ceil((resetsAt.getTime() - now) / 1000)) : null;
			return apiError(
				429,
				admission.code,
				REFUSALS[admission.code],
				{
					// The SDKs retry a 429 themselves, and don't wait out hours.
					'x-should-retry': 'false',
					...(wait ? { 'retry-after': String(wait) } : {}),
					...usageHeader(account, now)
				},
				{ resets_at: resetsAt?.toISOString() ?? null }
			);
		}

		this.going.set(userId, (this.going.get(userId) ?? 0) + 1);
		let released = false;
		const release = () => {
			if (released) return;
			released = true;
			this.going.set(userId, (this.going.get(userId) ?? 1) - 1);
		};

		let response: Response;
		try {
			response = await this.openrouter.forward(
				PATHS[endpoint],
				endpoint === 'chat' ? capped(body, json) : body,
				// A picture is made, and paid for at OpenRouter, even when whoever asked goes away
				// before it's back: it goes on to the end, and is charged like any other.
				endpoint === 'image' ? undefined : signal
			);
		} catch (err) {
			release();
			if (signal?.aborted) return new Response(null, { status: 499 });
			this.log.error('[nolune api] OpenRouter could not be reached:', err);
			return apiError(
				502,
				'upstream_unreachable',
				"nolune's API couldn't reach the model just now."
			);
		}

		const settle = (cost: number | null, id: string | null) =>
			this.settle(userId, request, admission.paidBy, now, cost, id);

		if (!response.ok) {
			release();
			const text = await response.text();
			// 401 and 402 are about nolune's own key or credits, not the family's request.
			if (response.status === 401 || response.status === 402) {
				this.log.error(
					`[nolune api] OpenRouter refused nolune's key (${response.status}): ${text}`
				);
				return apiError(
					503,
					'upstream_unavailable',
					"nolune's API can't reach models just now. Try again later."
				);
			}
			return new Response(text, {
				status: response.status,
				headers: { 'content-type': response.headers.get('content-type') ?? 'application/json' }
			});
		}

		if (endpoint === 'chat' && json.stream === true && response.body) {
			const body = tapStream(response.body, (end) => {
				release();
				this.track(settle(costOf(end.usage), end.id));
			});
			return new Response(body, {
				status: response.status,
				headers: {
					'content-type': response.headers.get('content-type') ?? 'text/event-stream',
					'cache-control': 'no-cache',
					...usageHeader(account, now)
				}
			});
		}

		const text = await response.text();
		release();
		let reply: { id?: unknown; usage?: unknown } = {};
		try {
			reply = JSON.parse(text);
		} catch {
			// Passed on as it came; there's nothing to charge by.
		}
		const after = await settle(
			costOf(reply.usage as Parameters<typeof costOf>[0]),
			// The Image API's replies carry no id; their headers do.
			typeof reply.id === 'string' ? reply.id : response.headers.get('x-generation-id')
		);
		return new Response(text, {
			status: response.status,
			headers: {
				'content-type': response.headers.get('content-type') ?? 'application/json',
				...usageHeader(after, this.now())
			}
		});
	}

	/** Why a picture request can't go: a model the plan doesn't offer, too many, or a stream. */
	private async imageRequestProblem(
		json: Record<string, unknown>,
		modelId: string
	): Promise<Response | null> {
		let model: ImageModel | undefined;
		try {
			model = (await this.openrouter.imageModels()).find((m) => m.id === modelId);
		} catch (err) {
			this.log.error("[nolune api] OpenRouter's image models could not be listed:", err);
			return apiError(
				502,
				'upstream_unreachable',
				"nolune's API couldn't reach the model just now."
			);
		}
		if (!model) {
			return apiError(
				400,
				'model_not_offered',
				`The nolune plan doesn't offer ${modelId || 'that model'} for pictures.`
			);
		}
		const n = json.n ?? 1;
		if (typeof n !== 'number' || !Number.isInteger(n) || n < 1 || n > MAX_IMAGES) {
			return apiError(
				400,
				'too_many_images',
				`Ask for between 1 and ${MAX_IMAGES} pictures at a time.`
			);
		}
		if (json.stream === true) {
			return apiError(
				400,
				'invalid_request',
				"The nolune plan's pictures come whole, not streamed."
			);
		}
		return null;
	}

	/** Charges a request that's done; one with no cost yet is charged once OpenRouter knows it. */
	private async settle(
		userId: string,
		request: PlanRequest,
		paidBy: Extract<Admission, { ok: true }>['paidBy'],
		at: number,
		cost: number | null,
		id: string | null
	): Promise<Account | null> {
		const charged = (dollars: number) =>
			updateAccount(this.db, userId, (account) =>
				account ? charge(account, { request, paidBy, at, cost: micros(dollars) }) : null
			);
		if (cost !== null) return charged(cost);
		if (!id) {
			this.log.warn(`[nolune api] a request of ${userId} ended with no cost and no generation id`);
			return readAccount(this.db, userId);
		}
		this.track(
			(async () => {
				for (const wait of this.settleAfter) {
					await new Promise((resolve) => setTimeout(resolve, wait));
					const found = await this.openrouter.generationCost(id).catch(() => null);
					if (found !== null) return charged(found);
				}
				this.log.warn(`[nolune api] never learned what ${id} cost; ${userId} wasn't charged`);
			})()
		);
		return readAccount(this.db, userId);
	}
}
