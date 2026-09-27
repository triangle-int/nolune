import { apiKeyHelp, configuredApiKey } from './config.ts';
import type { GeneratedImage, ImageRequest, ImageShape } from './image-generation.ts';

/*
 * OpenAI's Image API, for `btw generate image`. No SDK (chats use it, in openai-chat.ts): it's
 * two endpoints, and a plain fetch spares the command loading it. Generations take a prompt;
 * edits take the same fields plus the input images, as multipart.
 */

/** OPENAI_BASE_URL, as in OpenAI's own SDKs, points it at a proxy or a compatible server. */
export function openaiBaseUrl(): string {
	return (process.env.OPENAI_BASE_URL || 'https://api.openai.com/v1').replace(/\/+$/, '');
}

function apiUrl(endpoint: 'generations' | 'edits'): string {
	return `${openaiBaseUrl()}/images/${endpoint}`;
}
/** High quality at large sizes can take a couple of minutes. */
const TIMEOUT_MS = 5 * 60_000;

export const OPENAI_QUALITIES = ['low', 'medium', 'high', 'xhigh', 'max', 'auto'] as const;
/** The API takes at most 16 input images per edit. */
export const OPENAI_MAX_INPUT_IMAGES = 16;

const SIZES: Record<Exclude<ImageShape, 'auto'>, string> = {
	square: '1024x1024',
	portrait: '1024x1536',
	landscape: '1536x1024'
};

export class OpenAIError extends Error {}

function sizeParam(size: ImageRequest['size']): string {
	if (typeof size === 'object') return `${size.width}x${size.height}`;
	return size === 'auto' ? 'auto' : SIZES[size];
}

/** The fields both endpoints share. Options left unset are left out, so the model's defaults apply. */
function fields(request: ImageRequest): Record<string, string> {
	const out: Record<string, string> = {
		model: request.model,
		prompt: request.prompt,
		n: String(request.count),
		size: sizeParam(request.size)
	};
	if (request.quality) out.quality = request.quality;
	if (request.background) out.background = request.background;
	if (request.format) out.output_format = request.format;
	return out;
}

interface ApiResponse {
	data?: { b64_json?: string }[];
	output_format?: string;
	error?: { message?: string; code?: string | null; type?: string };
}

async function describeFailure(res: Response): Promise<string> {
	let body: ApiResponse | null = null;
	try {
		body = (await res.json()) as ApiResponse;
	} catch {
		// not JSON
	}
	const message = body?.error?.message?.trim() || res.statusText || 'no details';
	const code = body?.error?.code ?? '';
	if (res.status === 401) {
		return `OpenAI didn't accept the API key. ${apiKeyHelp('openai')}`;
	}
	if (code === 'moderation_blocked' || code === 'content_policy_violation') {
		return `OpenAI's safety system refused this request: ${message}`;
	}
	if (res.status === 429 && !/quota|billing/i.test(message)) {
		return 'Rate limited by OpenAI. Try again shortly.';
	}
	return `OpenAI API error ${res.status}: ${message}`;
}

export async function generateWithOpenAI(request: ImageRequest): Promise<GeneratedImage[]> {
	const key = configuredApiKey('openai')?.key;
	if (!key) throw new OpenAIError(`No OpenAI API key. ${apiKeyHelp('openai')}`);

	const signal = AbortSignal.any([request.signal, AbortSignal.timeout(TIMEOUT_MS)]);
	const headers = { authorization: `Bearer ${key}` };
	let res: Response;
	try {
		if (request.images.length === 0) {
			res = await fetch(apiUrl('generations'), {
				method: 'POST',
				headers: { ...headers, 'content-type': 'application/json' },
				body: JSON.stringify({ ...fields(request), n: request.count }),
				signal
			});
		} else {
			const form = new FormData();
			for (const [name, value] of Object.entries(fields(request))) form.append(name, value);
			// Several images go as `image[]`; one as `image`.
			const field = request.images.length > 1 ? 'image[]' : 'image';
			for (const image of request.images) {
				form.append(
					field,
					new Blob([new Uint8Array(image.data)], { type: image.mediaType }),
					image.name
				);
			}
			res = await fetch(apiUrl('edits'), { method: 'POST', headers, body: form, signal });
		}
	} catch (err) {
		const e = err as { name?: string; cause?: { code?: string; message?: string } };
		if (e.name === 'TimeoutError') {
			throw new OpenAIError(`OpenAI didn't answer within ${TIMEOUT_MS / 60_000} minutes.`, {
				cause: err
			});
		}
		if (e.name === 'AbortError') throw err;
		const why = e.cause?.code ?? e.cause?.message ?? (err as Error).message;
		throw new OpenAIError(`Couldn't reach OpenAI (${why}).`, { cause: err });
	}
	if (!res.ok) throw new OpenAIError(await describeFailure(res));

	const body = (await res.json()) as ApiResponse;
	const format = (body.output_format ?? request.format ?? 'png') as GeneratedImage['format'];
	const images = (body.data ?? []).flatMap((item) =>
		item.b64_json ? [{ data: Buffer.from(item.b64_json, 'base64'), format }] : []
	);
	if (!images.length) throw new OpenAIError('OpenAI answered without an image.');
	return images;
}
