/*
 * OpenRouter, behind nolune's API: the models the plan offers (for chats, and for pictures through
 * OpenRouter's Image API), requests passed on with nolune's key, and what each one cost.
 */

/** A model as OpenRouter's `GET /models` lists it; the gateway reads the same shape. */
export interface Model {
	id: string;
	context_length?: number | null;
	pricing?: Partial<Record<'prompt' | 'completion' | 'input_cache_read', string>>;
	supported_parameters?: string[];
	[key: string]: unknown;
}

/** An image model as OpenRouter's `GET /images/models` lists it, with what each request field takes. */
export interface ImageModel {
	id: string;
	architecture?: { input_modalities?: string[]; output_modalities?: string[] };
	supported_parameters?: Record<
		string,
		| { type: 'enum'; values: string[] }
		| { type: 'range'; min: number; max: number }
		| { type: 'boolean' }
	>;
	supports_streaming?: boolean;
	[key: string]: unknown;
}

/** What OpenRouter says a request used, in its responses and its stream's last chunk. */
export interface Usage {
	prompt_tokens?: number;
	completion_tokens?: number;
	cost?: number;
	is_byok?: boolean;
	cost_details?: { upstream_inference_cost?: number | null } | null;
}

/**
 * What a request cost nolune, in dollars: OpenRouter's charge, plus the provider's own bill when
 * the request went on a provider key of nolune's at OpenRouter (BYOK), which OpenRouter reports
 * (`upstream_inference_cost`) but doesn't charge. Null when nothing says.
 */
export function costOf(usage: Usage | null | undefined): number | null {
	if (!usage || typeof usage.cost !== 'number') return null;
	const upstream = usage.is_byok ? (usage.cost_details?.upstream_inference_cost ?? 0) : 0;
	return usage.cost + upstream;
}

const HOUR = 60 * 60 * 1000;

export interface OpenRouterOptions {
	apiKey: string;
	baseURL?: string;
	/** For tests. */
	fetch?: typeof fetch;
}

export class OpenRouter {
	private readonly apiKey: string;
	private readonly baseURL: string;
	private readonly fetch: typeof fetch;
	private readonly listed = new Map<string, { at: number; models: Promise<unknown[]> }>();

	constructor({ apiKey, baseURL, fetch: fetchImpl }: OpenRouterOptions) {
		this.apiKey = apiKey;
		this.baseURL = (baseURL || 'https://openrouter.ai/api/v1').replace(/\/+$/, '');
		this.fetch = fetchImpl ?? fetch;
	}

	private headers(): Record<string, string> {
		// OpenRouter credits the app on its rankings by these.
		return {
			authorization: `Bearer ${this.apiKey}`,
			'http-referer': 'https://nolune.dev',
			'x-title': 'nolune'
		};
	}

	/** Every model OpenRouter lists, kept for an hour; the last list stays while a new one fails. */
	models(now = Date.now()): Promise<Model[]> {
		return this.catalog<Model>('/models', now);
	}

	/** Every image model, from the Image API's own list, kept the same way. */
	imageModels(now = Date.now()): Promise<ImageModel[]> {
		return this.catalog<ImageModel>('/images/models', now);
	}

	private catalog<T>(path: string, now: number): Promise<T[]> {
		const kept = this.listed.get(path);
		if (kept && now - kept.at < HOUR) return kept.models as Promise<T[]>;
		const models = this.list<T>(path).catch((err) => {
			if (kept) return kept.models as Promise<T[]>;
			this.listed.delete(path);
			throw err;
		});
		this.listed.set(path, { at: now, models });
		return models;
	}

	private async list<T>(path: string): Promise<T[]> {
		const response = await this.fetch(`${this.baseURL}${path}`, {
			headers: this.headers(),
			signal: AbortSignal.timeout(20_000)
		});
		if (!response.ok) throw new Error(`OpenRouter's ${path.slice(1)}: ${response.status}`);
		const { data } = (await response.json()) as { data: T[] };
		return data;
	}

	/** Passes a request on, as it is apart from the key. */
	forward(
		path: '/chat/completions' | '/embeddings' | '/images',
		body: string,
		signal?: AbortSignal
	) {
		return this.fetch(`${this.baseURL}${path}`, {
			method: 'POST',
			headers: { ...this.headers(), 'content-type': 'application/json' },
			body,
			signal
		});
	}

	/**
	 * What a generation cost, asked after the fact (a stream cut off before its usage came). Null
	 * while OpenRouter doesn't have it yet, which takes it a few seconds to a minute.
	 */
	async generationCost(id: string): Promise<number | null> {
		const response = await this.fetch(`${this.baseURL}/generation?id=${encodeURIComponent(id)}`, {
			headers: this.headers(),
			signal: AbortSignal.timeout(20_000)
		});
		if (response.status === 404) return null;
		if (!response.ok) throw new Error(`OpenRouter's generation ${id}: ${response.status}`);
		const { data } = (await response.json()) as {
			data: { total_cost?: number; is_byok?: boolean; upstream_inference_cost?: number | null };
		};
		if (typeof data.total_cost !== 'number') return null;
		return data.total_cost + (data.is_byok ? (data.upstream_inference_cost ?? 0) : 0);
	}
}

/** Models the plan offers for chats: those that call tools, since nolune's agent works through one. */
export function chatModels(models: Model[]): Model[] {
	return models.filter((model) => model.supported_parameters?.includes('tools'));
}

/** Embedding models the plan offers: OpenAI's, which nolune's memory search is tuned for. */
export const EMBEDDING_MODELS = ['openai/text-embedding-3-small', 'openai/text-embedding-3-large'];
