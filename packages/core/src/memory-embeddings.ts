import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type OpenAI from 'openai';
import { configuredApiKey, readConfig, updateConfig, type Config } from './config.ts';
import { findCustomProvider, openaiUrl, splitModel } from './custom-providers.ts';
import { openaiBaseUrl } from './openai.ts';
import { openrouterBaseUrl } from './openrouter.ts';
import { profileMemoryDir } from './paths.ts';

/*
 * Embeddings of memory facts, for finding them by meaning (memory-search.ts), from any
 * OpenAI-compatible embeddings API: by default OpenAI's or OpenRouter's, with the key nolune already
 * has, or a custom provider that speaks OpenAI's API (custom-providers.ts), like Ollama, LM Studio
 * or oMLX on this computer, for anyone who wants them local.
 * Each fact's vector is kept in a hidden file next to the notes and made again only when the fact
 * or the model changes. Facts are embedded in the background; a message waits only for its own.
 */

export const DEFAULT_EMBEDDING_MODEL = 'text-embedding-3-small';
/** OpenAI's model, as each provider names it. */
export const DEFAULT_EMBEDDING_MODELS = {
	openai: DEFAULT_EMBEDDING_MODEL,
	openrouter: `openai/${DEFAULT_EMBEDDING_MODEL}`
} as const;
/** Hidden, like the fact dates: `nolune memory` refuses names starting with a dot. */
const FILE = '.embeddings.json';
/** Facts per request: small enough for a server on a laptop. */
const BATCH = 64;
const BATCH_TIMEOUT_MS = 60_000;

export type EmbeddingSetting = NonNullable<Config['embeddings']>;
export type EmbeddingProvider = Exclude<EmbeddingSetting, 'off'>['provider'];
const EMBEDDING_PROVIDERS: EmbeddingProvider[] = ['openai', 'openrouter', 'custom-openai'];

/** Where embeddings come from. */
export interface EmbeddingSource {
	/** An OpenAI-compatible API, like https://api.openai.com/v1. */
	url: string;
	model: string;
	key: string | null;
	/** For people: `openai/text-embedding-3-small`, or a custom provider's `Ollama/nomic-embed-text`. */
	name: string;
}

function setting(): Config['embeddings'] {
	try {
		return readConfig().embeddings;
	} catch {
		return undefined;
	}
}

function fromProvider(provider: EmbeddingProvider, model: string): EmbeddingSource | null {
	if (provider === 'custom-openai') {
		// `<id>/<model>`, as a custom preset's.
		const { provider: id, model: bare } = splitModel(model);
		const custom = id ? findCustomProvider(id) : undefined;
		if (custom?.api !== 'openai' || !bare) return null;
		const name = `${custom.name}/${bare}`;
		return { url: openaiUrl(custom.url), model: bare, key: custom.key ?? null, name };
	}
	const found = configuredApiKey(provider);
	if (!found) return null;
	const url = provider === 'openai' ? openaiBaseUrl() : openrouterBaseUrl();
	return { url, model, key: found.key, name: `${provider}/${model}` };
}

/**
 * The configured source, or nolune's own choice when none is: OpenAI's model with OpenAI's key, else
 * the same model through OpenRouter. Null when it's off, or its key or custom provider is missing.
 */
export function embeddingSource(configured = setting()): EmbeddingSource | null {
	if (configured === 'off') return null;
	if (configured) return fromProvider(configured.provider, configured.model);
	return (
		fromProvider('openai', DEFAULT_EMBEDDING_MODELS.openai) ??
		fromProvider('openrouter', DEFAULT_EMBEDDING_MODELS.openrouter)
	);
}

/** What's set and what's in use, for Models & keys. */
export interface EmbeddingState {
	/** `auto` when nothing is set. */
	mode: 'auto' | 'off' | EmbeddingProvider;
	/** The model set for a provider. */
	model: string | null;
	/** What search by meaning uses now, as EmbeddingSource names it; null: words only. */
	using: string | null;
}

export function embeddingState(): EmbeddingState {
	const configured = setting();
	const using = embeddingSource(configured)?.name ?? null;
	if (configured === undefined) return { mode: 'auto', model: null, using };
	if (configured === 'off') return { mode: 'off', model: null, using };
	return { mode: configured.provider, model: configured.model, using };
}

/** Saves where embeddings come from; undefined is auto. A new source's vectors are made anew. */
export function saveEmbeddingSetting(value: EmbeddingSetting | undefined): void {
	updateConfig((c) => {
		if (value === undefined) delete c.embeddings;
		else c.embeddings = value;
	});
}

/** In words, for `nolune config`: what memory search uses for meaning, or why nothing. */
export function embeddingStatus(): string {
	const configured = setting();
	if (configured === 'off') return 'off (nolune config set embeddings auto turns it on)';
	const source = embeddingSource(configured);
	if (source) return source.name;
	if (configured?.provider === 'custom-openai') {
		const { provider: id } = splitModel(configured.model);
		return `${configured.provider}/${configured.model}, but there is no custom provider "${id}" that speaks OpenAI's API (nolune provider list)`;
	}
	if (configured) {
		return `${configured.provider}/${configured.model}, but there is no ${configured.provider} key`;
	}
	return "off: no OpenAI or OpenRouter key (or a custom provider's model: nolune config set embeddings custom-openai/<provider>/<model>)";
}

/**
 * `nolune config set embeddings`: `auto`, `off`, or `<provider>/<model>`, where the provider is
 * openai, openrouter or custom-openai, whose model is `<id>/<model>` on a custom provider
 * `nolune provider add` saved that speaks OpenAI's API. Unset means auto.
 */
export function parseEmbeddingSetting(value: string): EmbeddingSetting | undefined {
	const word = value.trim();
	if (word === 'auto') return undefined;
	if (word === 'off') return 'off';
	if (/^https?:\/\//i.test(word)) {
		throw new Error(
			'add it with nolune provider add <name> <url>; then nolune config set embeddings custom-openai/<provider>/<model>'
		);
	}
	const slash = word.indexOf('/');
	const provider = word.slice(0, slash) as EmbeddingProvider;
	const model = word.slice(slash + 1);
	if (slash < 0 || !EMBEDDING_PROVIDERS.includes(provider) || !model) {
		throw new Error(
			'embeddings are auto, off, openai/<model>, openrouter/<model>, or custom-openai/<provider>/<model> for a model of a custom provider (Ollama, LM Studio...)'
		);
	}
	if (provider === 'custom-openai') {
		const on = splitModel(model);
		if (!on.provider || !on.model) {
			throw new Error(
				'give the custom provider too: custom-openai/<provider>/<model> (nolune provider list)'
			);
		}
		const custom = findCustomProvider(on.provider);
		if (!custom) throw new Error(`no custom provider "${on.provider}" (nolune provider list)`);
		if (custom.api !== 'openai') {
			throw new Error(`${custom.name} speaks Anthropic's API, which has no embeddings`);
		}
		return { provider, model: `${custom.id}/${on.model}` };
	}
	return { provider, model };
}

// --- Asking for them ---

type Sdk = typeof import('openai');
/** Loaded on first use, like the chats' (openai-chat.ts): most `nolune` commands never need it. */
let sdk: Sdk | undefined;
let cachedClient: { url: string; key: string; client: OpenAI } | undefined;

async function client(source: EmbeddingSource): Promise<OpenAI> {
	// A local server takes no key, but the SDK wants one.
	const key = source.key ?? 'none';
	if (!cachedClient || cachedClient.url !== source.url || cachedClient.key !== key) {
		const { OpenAI: Client } = (sdk ??= await import('openai'));
		const client = new Client({ apiKey: key, baseURL: source.url });
		cachedClient = { url: source.url, key, client };
	}
	return cachedClient.client;
}

/** Unit vectors for `texts`, in order. */
export async function embed(
	source: EmbeddingSource,
	texts: string[],
	timeoutMs: number,
	retries = 1
): Promise<Float32Array[]> {
	const api = await client(source);
	const response = await api.embeddings.create(
		{ model: source.model, input: texts, encoding_format: 'float' },
		{ timeout: timeoutMs, maxRetries: retries }
	);
	const data = [...(response.data ?? [])].sort((a, b) => a.index - b.index);
	if (data.length !== texts.length) {
		throw new Error(`${source.name} returned ${data.length} embeddings for ${texts.length} texts`);
	}
	return data.map((item) => {
		const vector = Float32Array.from(item.embedding as number[]);
		let length = 0;
		for (const x of vector) length += x * x;
		length = Math.sqrt(length);
		if (!vector.length || !length) throw new Error(`${source.name} returned an empty embedding`);
		return vector.map((x) => x / length);
	});
}

/** What's wrong with the configured source, asking it once; null when it works or there is none. */
export async function embeddingProblem(): Promise<string | null> {
	const source = embeddingSource();
	if (!source) return null;
	try {
		await embed(source, ['test'], 20_000);
		return null;
	} catch (err) {
		return describe(err);
	}
}

/** Both are unit vectors. */
export function similarity(a: Float32Array, b: Float32Array): number {
	let sum = 0;
	for (let i = 0; i < a.length && i < b.length; i++) sum += a[i] * b[i];
	return sum;
}

// --- The vectors of a profile's facts ---

interface Stored {
	/** The source they came from: vectors of another model can't be compared. */
	source: string;
	vectors: Map<string, Float32Array>;
}

const sourceId = (source: EmbeddingSource) => `${source.url} ${source.model}`;
const hash = (text: string) => createHash('sha256').update(text).digest('base64url').slice(0, 22);

function file(slug: string): string {
	return join(profileMemoryDir(slug), FILE);
}

/** What's saved, when it's from `source`; nothing when it can't be read. */
function load(slug: string, source: EmbeddingSource): Stored {
	const empty: Stored = { source: sourceId(source), vectors: new Map() };
	try {
		if (!existsSync(file(slug))) return empty;
		const saved = JSON.parse(readFileSync(file(slug), 'utf8')) as {
			version?: unknown;
			source?: unknown;
			vectors?: unknown;
		};
		if (saved.version !== 1 || saved.source !== empty.source || !Array.isArray(saved.vectors)) {
			return empty;
		}
		for (const entry of saved.vectors as unknown[]) {
			if (!Array.isArray(entry) || typeof entry[0] !== 'string' || typeof entry[1] !== 'string') {
				continue;
			}
			const bytes = Buffer.from(entry[1], 'base64');
			const vector = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 4);
			empty.vectors.set(entry[0], Float32Array.from(vector));
		}
	} catch {
		// Broken: made again.
	}
	return empty;
}

function save(slug: string, stored: Stored): void {
	const dir = profileMemoryDir(slug);
	const temp = join(dir, `.tmp-${randomUUID()}`);
	try {
		mkdirSync(dir, { recursive: true, mode: 0o700 });
		const vectors = [...stored.vectors].map(([key, vector]) => [
			key,
			Buffer.from(vector.buffer, vector.byteOffset, vector.byteLength).toString('base64')
		]);
		writeFileSync(temp, JSON.stringify({ version: 1, source: stored.source, vectors }), {
			mode: 0o600
		});
		renameSync(temp, file(slug));
	} catch (err) {
		console.error(`[nolune] ${slug} could not save memory embeddings:`, err);
	} finally {
		rmSync(temp, { force: true });
	}
}

/** The saved vectors of `texts` that have one, and the texts that don't. */
export function savedVectors(
	slug: string,
	source: EmbeddingSource,
	texts: string[]
): { vectors: Map<string, Float32Array>; missing: string[] } {
	const stored = load(slug, source);
	const vectors = new Map<string, Float32Array>();
	const missing: string[] = [];
	for (const text of texts) {
		const vector = stored.vectors.get(hash(text));
		if (vector) vectors.set(text, vector);
		else missing.push(text);
	}
	return { vectors, missing };
}

const updates = new Map<string, { again: boolean; done: Promise<void> }>();

/**
 * Embeds the texts that have no vector yet and forgets the ones no longer in `texts`, which are
 * all of the profile's facts. One at a time per profile; a call while one runs waits for it and
 * then catches up.
 */
export function updateEmbeddings(slug: string, texts: () => string[]): Promise<void> {
	const running = updates.get(slug);
	if (running) {
		running.again = true;
		return running.done;
	}
	const state = { again: false, done: Promise.resolve() };
	state.done = (async () => {
		do {
			state.again = false;
			await update(slug, texts()).catch((err: unknown) => {
				console.error(`[nolune] ${slug} could not embed memory:`, describe(err));
			});
		} while (state.again);
	})().finally(() => updates.delete(slug));
	updates.set(slug, state);
	return state.done;
}

async function update(slug: string, texts: string[]): Promise<void> {
	const source = embeddingSource();
	if (!source) return;
	const stored = load(slug, source);
	const wanted = new Map(texts.map((text) => [hash(text), text]));
	const missing = [...wanted].filter(([key]) => !stored.vectors.has(key));
	let changed = false;
	for (const key of stored.vectors.keys()) {
		if (wanted.has(key)) continue;
		stored.vectors.delete(key);
		changed = true;
	}
	for (let i = 0; i < missing.length; i += BATCH) {
		const batch = missing.slice(i, i + BATCH);
		const vectors = await embed(
			source,
			batch.map(([, text]) => text),
			BATCH_TIMEOUT_MS
		);
		batch.forEach(([key], j) => stored.vectors.set(key, vectors[j]));
		changed = true;
		// Saved after each batch: a big memory is usable before the last one is done.
		save(slug, stored);
	}
	if (changed && !missing.length) save(slug, stored);
	if (missing.length) {
		console.log(`[nolune] ${slug} embedded ${missing.length} memory facts with ${source.name}`);
	}
}

/** An API error in one line, without the stack. */
export function describe(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}
