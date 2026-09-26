import { readFileSync, statSync } from 'node:fs';
import { basename, extname } from 'node:path';
import { readConfig } from './config.ts';
import { inspectImage, prepareImage, stripJpegMetadata, type MediaType } from './images.ts';
import {
	OPENAI_MAX_INPUT_IMAGES,
	OPENAI_QUALITIES,
	generateWithOpenAI,
	openaiApiKey
} from './openai.ts';

/*
 * `btw generate image`: making pictures with an image model. The agent runs it like any other
 * command, so the gateway has no code for it. Each provider is a module with the same shape
 * (openai.ts today); openrouter, fal or higgsfield would each add one and an entry in PROVIDERS.
 */

export const IMAGE_PROVIDERS = ['openai'] as const;
export type ImageProvider = (typeof IMAGE_PROVIDERS)[number];

export const DEFAULT_IMAGE_MODEL = 'openai/gpt-image-2.5-flare';

/** The shapes every provider understands. Providers map them to their own sizes. */
export const IMAGE_SHAPES = ['square', 'portrait', 'landscape', 'auto'] as const;
export type ImageShape = (typeof IMAGE_SHAPES)[number];

export const IMAGE_FORMATS = ['png', 'jpeg', 'webp'] as const;
export type ImageFormat = (typeof IMAGE_FORMATS)[number];

export const IMAGE_BACKGROUNDS = ['auto', 'transparent', 'opaque'] as const;
export type ImageBackground = (typeof IMAGE_BACKGROUNDS)[number];

/** More than this per command is almost always a mistake, and every picture costs money. */
export const MAX_IMAGE_COUNT = 4;

export interface InputImage {
	name: string;
	data: Buffer;
	mediaType: MediaType;
}

/** One call to a provider, with the choices already checked. */
export interface ImageRequest {
	/** The provider's own model id. */
	model: string;
	prompt: string;
	/** Pictures to edit or to use as references. Empty: make one from the prompt alone. */
	images: InputImage[];
	size: ImageShape | { width: number; height: number };
	/** Left out: the model's default. */
	quality?: string;
	background?: ImageBackground;
	format?: ImageFormat;
	count: number;
	signal: AbortSignal;
}

export interface GeneratedImage {
	data: Buffer;
	format: ImageFormat;
}

interface ProviderModule {
	/** The provider's name for people. */
	label: string;
	/** How the person adds the key, for messages. */
	keyCommand: string;
	hasKey: () => boolean;
	qualities: readonly string[];
	maxInputImages: number;
	/** Formats it accepts for input images; others are converted first. */
	inputTypes: readonly MediaType[];
	generate: (request: ImageRequest) => Promise<GeneratedImage[]>;
}

const PROVIDERS: Record<ImageProvider, ProviderModule> = {
	openai: {
		label: 'OpenAI',
		keyCommand: 'btw key set openai',
		hasKey: () => openaiApiKey() !== null,
		qualities: OPENAI_QUALITIES,
		maxInputImages: OPENAI_MAX_INPUT_IMAGES,
		inputTypes: ['image/png', 'image/jpeg', 'image/webp'],
		generate: generateWithOpenAI
	}
};

function isProvider(value: string): value is ImageProvider {
	return (IMAGE_PROVIDERS as readonly string[]).includes(value);
}

/**
 * `openai/gpt-image-2.5-flare` → provider and model. Only the first segment can name the provider,
 * so model ids that contain slashes themselves (OpenRouter's, fal's) keep them. Without a known
 * provider in front, the model belongs to `defaultProvider`.
 */
export function parseImageModel(
	value: string,
	defaultProvider?: ImageProvider
): { provider: ImageProvider; model: string } {
	const trimmed = value.trim();
	const slash = trimmed.indexOf('/');
	const first = slash === -1 ? '' : trimmed.slice(0, slash);
	if (isProvider(first) && trimmed.length > slash + 1) {
		return { provider: first, model: trimmed.slice(slash + 1) };
	}
	if (!defaultProvider || !trimmed) {
		throw new Error(
			`Image models are written as <provider>/<model>, e.g. ${DEFAULT_IMAGE_MODEL}. Providers: ${IMAGE_PROVIDERS.join(', ')}.`
		);
	}
	return { provider: defaultProvider, model: trimmed };
}

/** The provider of the configured model, for model ids given without one. */
function configuredProvider(): ImageProvider {
	try {
		return parseImageModel(configuredImageModel()).provider;
	} catch {
		return parseImageModel(DEFAULT_IMAGE_MODEL).provider;
	}
}

/**
 * `<provider>/<model>` for a model given with or without its provider, after checking that the
 * provider has an API key. Throws saying how to add one.
 */
export function checkImageModel(value?: string): string {
	const { provider, model } = parseImageModel(
		value?.trim() || configuredImageModel(),
		configuredProvider()
	);
	const entry = PROVIDERS[provider];
	if (!entry.hasKey()) {
		throw new Error(`No ${entry.label} API key. Set it with \`${entry.keyCommand}\`.`);
	}
	return `${provider}/${model}`;
}

/** The configured image model, or the default. */
export function configuredImageModel(): string {
	try {
		return readConfig().imageModel || DEFAULT_IMAGE_MODEL;
	} catch {
		return DEFAULT_IMAGE_MODEL;
	}
}

export interface ImageGenerationStatus {
	/** `<provider>/<model>`. */
	model: string;
	ready: boolean;
	/** What's missing, in plain words, when it isn't ready. */
	problem: string | null;
}

/** Whether `btw generate image` can work, for the Images page and `btw config`. */
export function imageGenerationStatus(): ImageGenerationStatus {
	const model = configuredImageModel();
	let provider: ProviderModule;
	try {
		provider = PROVIDERS[parseImageModel(model).provider];
	} catch (err) {
		return { model, ready: false, problem: (err as Error).message };
	}
	if (!provider.hasKey()) {
		return { model, ready: false, problem: `No API key yet: run \`${provider.keyCommand}\`.` };
	}
	return { model, ready: true, problem: null };
}

/** `portrait`, `auto` or `1536x1024`. */
export function parseImageSize(value: string): ImageRequest['size'] {
	const shape = value.trim().toLowerCase();
	if ((IMAGE_SHAPES as readonly string[]).includes(shape)) return shape as ImageShape;
	const match = /^(\d{2,5})\s*[x×]\s*(\d{2,5})$/.exec(shape);
	if (!match) {
		throw new Error(`Size is ${IMAGE_SHAPES.join(', ')} or WIDTHxHEIGHT (like 1536x1024).`);
	}
	return { width: Number(match[1]), height: Number(match[2]) };
}

/** Largest input image sent as it is. OpenAI takes up to 50 MB. */
const MAX_INPUT_BYTES = 25 * 1024 * 1024;

/**
 * Reads an input image. PNG, JPEG and WebP go as they are, JPEGs without their EXIF (it carries
 * GPS positions); other formats, sideways photos and huge files are converted like `btw view`'s.
 */
function readInputImage(path: string, accept: readonly MediaType[]): InputImage {
	let data: Buffer;
	try {
		if (statSync(path).isDirectory()) throw new Error("it's a folder");
		data = readFileSync(path);
	} catch (err) {
		const code = (err as { code?: string }).code;
		throw new Error(
			`Can't read ${path}: ${code === 'ENOENT' ? 'no such file' : (err as Error).message}.`,
			{ cause: err }
		);
	}
	const name = basename(path);
	const info = inspectImage(data);
	if (
		info &&
		accept.includes(info.mediaType) &&
		info.orientation <= 1 &&
		data.length <= MAX_INPUT_BYTES
	) {
		const clean = info.mediaType === 'image/jpeg' ? stripJpegMetadata(data) : data;
		return { name, data: clean, mediaType: info.mediaType };
	}
	try {
		const prepared = prepareImage(path, accept);
		const ext = prepared.info.mediaType.slice('image/'.length);
		return {
			name: `${basename(name, extname(name))}.${ext === 'jpeg' ? 'jpg' : ext}`,
			data: prepared.data,
			mediaType: prepared.info.mediaType
		};
	} catch (err) {
		throw new Error(`Can't use ${path}: ${(err as Error).message}.`, { cause: err });
	}
}

export interface GenerateOptions {
	prompt: string;
	/** Paths of pictures to edit or use as references. */
	images?: string[];
	/** `<provider>/<model>`; defaults to the configured one. */
	model?: string;
	size?: ImageRequest['size'];
	quality?: string;
	background?: ImageBackground;
	format?: ImageFormat;
	count?: number;
	signal?: AbortSignal;
	/** Called once everything is checked and the input images are read, before the provider. */
	onStart?: (model: string) => void;
}

export async function generateImages(options: GenerateOptions): Promise<{
	model: string;
	images: GeneratedImage[];
}> {
	const model = checkImageModel(options.model);
	const { provider: name, model: providerModel } = parseImageModel(model);
	const provider = PROVIDERS[name];

	const prompt = options.prompt.trim();
	if (!prompt) throw new Error('Say what to make: the prompt is empty.');
	const count = options.count ?? 1;
	if (!Number.isInteger(count) || count < 1 || count > MAX_IMAGE_COUNT) {
		throw new Error(`Make between 1 and ${MAX_IMAGE_COUNT} images at a time.`);
	}
	if (options.quality && !provider.qualities.includes(options.quality)) {
		throw new Error(`Quality is one of ${provider.qualities.join(', ')}.`);
	}
	if (options.background === 'transparent' && options.format === 'jpeg') {
		throw new Error('A transparent background needs png or webp, not jpeg.');
	}
	const paths = options.images ?? [];
	if (paths.length > provider.maxInputImages) {
		throw new Error(`${provider.label} takes at most ${provider.maxInputImages} input images.`);
	}

	const inputs = paths.map((path) => readInputImage(path, provider.inputTypes));
	options.onStart?.(model);
	const images = await provider.generate({
		model: providerModel,
		prompt,
		images: inputs,
		size: options.size ?? 'auto',
		quality: options.quality,
		background: options.background,
		format: options.format,
		count,
		signal: options.signal ?? new AbortController().signal
	});
	return { model, images };
}
