import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { GeneratedImage, ImageFormat, ImageRequest, ImageShape } from './image-generation.ts';
import { inspectImage } from './images.ts';
import { openrouterBaseUrl } from './openrouter.ts';

/*
 * OpenRouter's Image API (`POST /images`), for `nolune generate image`: with an OpenRouter key, and
 * on the nolune plan, whose API takes the same requests (an `ImageApi` each, as chats have a
 * `ChatApi`). Models differ in what they take, so a request is fitted to the model's own list of
 * parameters (`GET /images/models`) before it goes: the nearest shape it makes, as many requests
 * as its `n` needs, and a clear no for what it can't do. Plain fetch, as in openai.ts.
 */

/** Where pictures are made, and how to ask. */
export interface ImageApi {
	provider: 'openrouter' | 'nolune-plan';
	/** For people: "OpenRouter", "the nolune plan". */
	label: string;
	baseURL: () => string;
	/** The bearer token; throws saying how to get one. */
	key: () => string;
	/** More headers for each request (the plan's: who it's for). */
	headers?: () => Record<string, string>;
	/** What a failed request means, in words, from its status and its body's `error`. */
	failure: (status: number, error: ApiError | undefined) => Error;
	/** After each request, done or not. */
	after?: () => void;
}

export interface ApiError {
	code?: string | number | null;
	message?: string;
	[key: string]: unknown;
}

type Parameter =
	| { type: 'enum'; values: string[] }
	| { type: 'range'; min: number; max: number }
	| { type: 'boolean' };

/** An image model as the Image API lists it. */
export interface ImageModel {
	id: string;
	architecture?: { input_modalities?: string[] };
	supported_parameters?: Record<string, Parameter>;
}

/** Every quality any model takes; each model takes its own (`quality` in its parameters). */
export const IMAGE_API_QUALITIES = ['auto', 'low', 'medium', 'high', 'xhigh', 'max'] as const;
/** The most any model takes; each model says its own (`input_references`). */
export const IMAGE_API_MAX_INPUT_IMAGES = 16;
/** High quality at large sizes can take a couple of minutes. */
const TIMEOUT_MS = 5 * 60_000;
const LIST_TIMEOUT_MS = 30_000;
const HOUR = 60 * 60 * 1000;

export class ImageApiError extends Error {}

/** OpenRouter's, with its key. */
export const OPENROUTER_IMAGES: ImageApi = {
	provider: 'openrouter',
	label: 'OpenRouter',
	baseURL: openrouterBaseUrl,
	key: () => {
		const key = configuredApiKey('openrouter')?.key;
		if (!key) throw new ImageApiError(`No OpenRouter API key. ${apiKeyHelp('openrouter')}`);
		return key;
	},
	headers: () => ({ 'http-referer': 'https://nolune.dev', 'x-title': 'nolune' }),
	failure: (status, error) => {
		const message = error?.message?.trim() || 'no details';
		if (status === 401) {
			return new ImageApiError(`OpenRouter didn't accept the API key. ${apiKeyHelp('openrouter')}`);
		}
		if (status === 402) {
			return new ImageApiError(`OpenRouter says the key is out of credits: ${message}`);
		}
		if (status === 403) return new ImageApiError(`OpenRouter refused this request: ${message}`);
		if (status === 429) return new ImageApiError('Rate limited by OpenRouter. Try again shortly.');
		if (status === 413) {
			return new ImageApiError('The pictures to start from are too large to send to OpenRouter.');
		}
		return new ImageApiError(`OpenRouter answered ${status}: ${message}`);
	}
};

const lists = new Map<string, { at: number; models: Promise<ImageModel[]> }>();

/** The API's image models, kept for an hour (the gateway asks often; a command, once). */
async function imageModels(api: ImageApi, signal: AbortSignal): Promise<ImageModel[]> {
	const url = `${api.baseURL()}/images/models`;
	const kept = lists.get(url);
	if (kept && Date.now() - kept.at < HOUR) return kept.models;
	const models = (async () => {
		const res = await call(api, url, { signal: anyOf(signal, LIST_TIMEOUT_MS) });
		const body = (await res.json()) as { data?: ImageModel[] };
		return body.data ?? [];
	})();
	lists.set(url, { at: Date.now(), models });
	models.catch(() => lists.delete(url));
	return models;
}

function anyOf(signal: AbortSignal, timeout: number): AbortSignal {
	return AbortSignal.any([signal, AbortSignal.timeout(timeout)]);
}

/** A request to the API (a GET, or a POST of `body`), with its errors in words. */
async function call(
	api: ImageApi,
	url: string,
	init: { body?: string; signal: AbortSignal }
): Promise<Response> {
	const headers = {
		authorization: `Bearer ${api.key()}`,
		...(init.body === undefined ? {} : { 'content-type': 'application/json' }),
		...api.headers?.()
	};
	let res: Response;
	try {
		res = await fetch(url, {
			method: init.body === undefined ? 'GET' : 'POST',
			headers,
			body: init.body,
			signal: init.signal
		});
	} catch (err) {
		const e = err as { name?: string; cause?: { code?: string; message?: string } };
		if (e.name === 'TimeoutError') {
			throw new ImageApiError(`${capital(api.label)} didn't answer in time.`, { cause: err });
		}
		if (e.name === 'AbortError') throw err;
		const why = e.cause?.code ?? e.cause?.message ?? (err as Error).message;
		throw new ImageApiError(`Couldn't reach ${api.label} (${why}).`, { cause: err });
	}
	if (!res.ok) {
		const body = (await res.json().catch(() => null)) as { error?: ApiError } | null;
		throw api.failure(res.status, body?.error);
	}
	return res;
}

function capital(text: string): string {
	return text[0].toUpperCase() + text.slice(1);
}

/** The shapes each one means, nearest first: models make different sets of them. */
const RATIOS: Record<Exclude<ImageShape, 'auto'>, string[]> = {
	square: ['1:1'],
	portrait: ['2:3', '3:4', '4:5', '9:16'],
	landscape: ['3:2', '4:3', '5:4', '16:9']
};

const FORMATS: Record<string, ImageFormat> = {
	'image/png': 'png',
	'image/jpeg': 'jpeg',
	'image/webp': 'webp'
};

function enumValues(parameter: Parameter | undefined): string[] | null {
	return parameter?.type === 'enum' ? parameter.values : null;
}

/**
 * The fields of one request for `model`, and how many pictures each request asks for. Throws
 * saying what the model can't do. Fields the model doesn't take are left out, so its defaults
 * apply.
 */
export function fitImageRequest(
	request: ImageRequest,
	model: ImageModel
): { body: Record<string, unknown>; counts: number[] } {
	const params = model.supported_parameters ?? {};
	const body: Record<string, unknown> = { model: model.id, prompt: request.prompt };

	if (request.images.length) {
		const takes = model.architecture?.input_modalities?.includes('image') ?? true;
		const most = params.input_references?.type === 'range' ? params.input_references.max : 0;
		if (!takes || most === 0) {
			throw new ImageApiError(`${model.id} makes pictures from words only: it takes no --image.`);
		}
		if (request.images.length > most) {
			throw new ImageApiError(`${model.id} takes at most ${most} input images.`);
		}
		body.input_references = request.images.map((image) => ({
			type: 'image_url',
			image_url: { url: `data:${image.mediaType};base64,${image.data.toString('base64')}` }
		}));
	}

	if (typeof request.size === 'object') {
		// Exact pixels, which the API fits to what the model makes.
		body.size = `${request.size.width}x${request.size.height}`;
	} else {
		const ratios = enumValues(params.aspect_ratio);
		const wanted = request.size === 'auto' ? ['auto'] : RATIOS[request.size];
		const ratio = ratios && wanted.find((r) => ratios.includes(r));
		if (ratio) body.aspect_ratio = ratio;
	}

	if (request.quality) {
		const qualities = enumValues(params.quality);
		if (!qualities) throw new ImageApiError(`${model.id} has no quality setting.`);
		if (!qualities.includes(request.quality)) {
			throw new ImageApiError(`${model.id}'s quality is one of ${qualities.join(', ')}.`);
		}
		body.quality = request.quality;
	}

	if (request.background) {
		const backgrounds = enumValues(params.background);
		if (backgrounds?.includes(request.background)) body.background = request.background;
		else if (request.background === 'transparent') {
			throw new ImageApiError(`${model.id} can't make a transparent background.`);
		}
	}

	// A format the model doesn't make is left to it: the pictures say what they are.
	if (request.format && enumValues(params.output_format)?.includes(request.format)) {
		body.output_format = request.format;
	}

	const most = params.n?.type === 'range' ? Math.max(1, params.n.max) : 1;
	const counts: number[] = [];
	for (let left = request.count; left > 0; left -= most) counts.push(Math.min(left, most));
	return { body, counts };
}

function decode(item: { b64_json?: string; media_type?: string }): GeneratedImage[] {
	if (!item.b64_json) return [];
	const data = Buffer.from(item.b64_json, 'base64');
	const format =
		FORMATS[item.media_type ?? ''] ??
		FORMATS[inspectImage(data)?.mediaType ?? ''] ??
		(item.media_type === 'image/svg+xml' ? null : 'png');
	if (!format) {
		throw new ImageApiError(
			'The model made a vector picture (SVG), which nolune saves only as PNG, JPEG or WebP. Pick another model.'
		);
	}
	return [{ data, format }];
}

/** Makes pictures with the API's `model`, fitted to what the model takes. */
export async function generateWithImageApi(
	api: ImageApi,
	request: ImageRequest
): Promise<GeneratedImage[]> {
	try {
		const models = await imageModels(api, request.signal);
		const model = models.find((m) => m.id === request.model);
		if (!model) {
			throw new ImageApiError(
				`${capital(api.label)} has no image model ${request.model}. Its image models are listed at https://openrouter.ai/models?output_modalities=image`
			);
		}
		const { body, counts } = fitImageRequest(request, model);
		const signal = anyOf(request.signal, TIMEOUT_MS);
		const batches = await Promise.all(
			counts.map(async (n) => {
				const res = await call(api, `${api.baseURL()}/images`, {
					body: JSON.stringify({ ...body, ...(n > 1 ? { n } : {}) }),
					signal
				});
				const reply = (await res.json()) as { data?: { b64_json?: string; media_type?: string }[] };
				return (reply.data ?? []).flatMap(decode);
			})
		);
		const images = batches.flat();
		if (!images.length)
			throw new ImageApiError(`${capital(api.label)} answered without a picture.`);
		return images;
	} finally {
		api.after?.();
	}
}
