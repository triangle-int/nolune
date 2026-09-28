import { createServer, type Server } from 'node:http';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { initConfig, updateConfig } from './config.ts';
import { removeCustomProvider, saveCustomProvider } from './custom-providers.ts';
import { addMemoryFact, writeMemoryNote } from './memory.ts';
import {
	embeddingSource,
	embeddingState,
	embeddingStatus,
	parseEmbeddingSetting,
	saveEmbeddingSetting
} from './memory-embeddings.ts';
import { embedMemory, recallFor, searchMemory, startEmbeddingMemory } from './memory-search.ts';
import { profileMemoryDir } from './paths.ts';
import { makeFamily } from './test/fixtures.ts';

/*
 * A stand-in for an OpenAI-compatible embeddings API. Its vectors come from a few concepts, so
 * "teeth" and "dentist" mean the same and "вайфай" means "wifi", while other text only shares a
 * dimension that tells nothing apart.
 */
const CONCEPTS: Record<string, string[]> = {
	dental: ['teeth', 'tooth', 'dentist'],
	net: ['wifi', 'вайфай', 'вайфая', 'internet'],
	secret: ['password', 'пароль'],
	pet: ['cat', 'vet', 'mochi']
};
const DIMENSIONS = [...Object.keys(CONCEPTS), 'rest'];

function vectorOf(text: string): number[] {
	const words = text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];
	const vector: number[] = DIMENSIONS.map((dim) =>
		dim === 'rest' ? 0 : words.filter((w) => CONCEPTS[dim].includes(w)).length
	);
	if (!vector.some(Boolean)) vector[DIMENSIONS.length - 1] = 1;
	return vector;
}

let server: Server;
let requests: { model: string; input: string[] }[] = [];
let failWith: number | null = null;

beforeAll(async () => {
	server = createServer((req, res) => {
		let body = '';
		req.on('data', (chunk) => (body += chunk));
		req.on('end', () => {
			const json = JSON.parse(body) as { model: string; input: string[] };
			requests.push(json);
			res.setHeader('content-type', 'application/json');
			if (req.url !== '/v1/embeddings' || failWith) {
				res.statusCode = failWith ?? 404;
				res.end(JSON.stringify({ error: { message: 'Not today' } }));
				return;
			}
			res.end(
				JSON.stringify({
					object: 'list',
					model: json.model,
					data: json.input.map((text, index) => ({
						object: 'embedding',
						index,
						embedding: vectorOf(text)
					})),
					usage: { prompt_tokens: 1, total_tokens: 1 }
				})
			);
		});
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	requests = [];
	failWith = null;
	const { port } = server.address() as { port: number };
	vi.stubEnv('OPENAI_BASE_URL', `http://127.0.0.1:${port}/v1`);
	vi.stubEnv('OPENAI_API_KEY', 'sk-test');
	vi.stubEnv('OPENROUTER_API_KEY', '');
	startEmbeddingMemory([]);
});

afterEach(() => {
	vi.unstubAllEnvs();
});

/** A dozen facts: enough for one to stand out. */
function family() {
	const { profile } = makeFamily();
	writeMemoryNote(
		profile.slug,
		'home',
		'# Home\n\n## Internet\n\n- Wifi password: mango42\n- Router in the attic\n'
	);
	writeMemoryNote(profile.slug, 'health', '# Health\n\n- Dentist: Dr. Keller, Elm Street 12\n');
	writeMemoryNote(
		profile.slug,
		'people/mia',
		'# Mia\n\n- Piano on Tuesdays\n- Riverside school, class 2B\n- Loves horses\n- Birthday on May 14\n'
	);
	writeMemoryNote(
		profile.slug,
		'routines',
		'# Routines\n\n- Pizza night is on Friday\n- Pancakes on Sunday\n- No mushrooms for Mia\n'
	);
	writeMemoryNote(profile.slug, 'plans', '# Plans\n\n- Rent is due on the 1st\n- Taxes in April\n');
	return profile;
}

describe('memory search by meaning', () => {
	it('finds facts that share no word with the message, in any language', async () => {
		const profile = family();
		await embedMemory(profile.slug);
		expect(requests).toEqual([
			{ model: 'text-embedding-3-small', input: expect.any(Array), encoding_format: 'float' }
		]);
		expect(requests[0].input).toHaveLength(12);
		expect(requests[0].input).toContain('health: Dentist: Dr. Keller, Elm Street 12');
		expect(requests[0].input).toContain('home › Internet: Wifi password: mango42');

		const teeth = await recallFor(profile.slug, 'Who fixes our teeth?', { known: '' });
		expect(teeth).toContain('- [health] Dentist: Dr. Keller, Elm Street 12');
		const wifi = await recallFor(profile.slug, 'Какой пароль от вайфая?', { known: '' });
		expect(wifi).toContain('- [home › Internet] Wifi password: mango42');
		expect(await recallFor(profile.slug, 'thanks!', { known: '' })).toBeNull();
		// Each message asked for its own embedding only: the facts' were kept.
		expect(requests.slice(1).map((r) => r.input)).toEqual([
			['Who fixes our teeth?'],
			['Какой пароль от вайфая?'],
			['thanks!']
		]);
		expect(existsSync(join(profileMemoryDir(profile.slug), '.embeddings.json'))).toBe(true);

		expect((await searchMemory(profile.slug, 'teeth')).map((hit) => hit.text)).toEqual([
			'Dentist: Dr. Keller, Elm Street 12'
		]);
	});

	it('embeds a new fact in the background; words find it meanwhile', async () => {
		const profile = family();
		await embedMemory(profile.slug);
		addMemoryFact(profile.slug, 'pets', 'Mochi goes to Dr. Lind every November');
		requests = [];

		const first = await recallFor(profile.slug, 'Book the vet for our cat', { known: '' });
		expect(first).toBeNull();
		await embedMemory(profile.slug);
		expect(requests.map((r) => r.input).sort()).toEqual([
			['Book the vet for our cat'],
			['pets: Mochi goes to Dr. Lind every November']
		]);
		const next = await recallFor(profile.slug, 'Book the vet for our cat', { known: '' });
		expect(next).toContain('Mochi goes to Dr. Lind every November');
	});

	it('falls back to words when the API fails, and asks nothing for a small memory', async () => {
		const profile = family();
		await embedMemory(profile.slug);
		failWith = 401;
		vi.spyOn(console, 'error').mockImplementation(() => {});
		expect(await recallFor(profile.slug, 'Who fixes our teeth?', { known: '' })).toBeNull();
		expect(await recallFor(profile.slug, 'wifi?', { known: '' })).toContain('mango42');

		const small = makeFamily('Ben').profile;
		addMemoryFact(small.slug, 'health', 'Dentist: Dr. Keller');
		requests = [];
		failWith = null;
		await embedMemory(small.slug);
		expect(await recallFor(small.slug, 'Who fixes our teeth?', { known: '' })).toBeNull();
		expect(requests.filter((r) => r.input.includes('Who fixes our teeth?'))).toEqual([]);
	});
});

describe('embeddingSource', () => {
	it("uses OpenAI's key, else OpenRouter's, unless set otherwise", () => {
		initConfig();
		expect(embeddingSource()).toMatchObject({
			model: 'text-embedding-3-small',
			key: 'sk-test',
			name: 'openai/text-embedding-3-small'
		});
		vi.stubEnv('OPENAI_API_KEY', '');
		vi.stubEnv('OPENROUTER_API_KEY', 'sk-or-test');
		vi.stubEnv('OPENROUTER_BASE_URL', 'http://127.0.0.1:9/api/v1');
		expect(embeddingSource()).toEqual({
			url: 'http://127.0.0.1:9/api/v1',
			model: 'openai/text-embedding-3-small',
			key: 'sk-or-test',
			name: 'openrouter/openai/text-embedding-3-small'
		});

		updateConfig((c) => (c.embeddings = parseEmbeddingSetting('off')));
		expect(embeddingSource()).toBeNull();
		updateConfig((c) => (c.embeddings = parseEmbeddingSetting('openai/text-embedding-3-large')));
		// Its key is missing.
		expect(embeddingSource()).toBeNull();
	});

	it("uses a custom provider's model, with its key when it has one", async () => {
		initConfig();
		// Added as the server itself: its OpenAI API is under /v1.
		const { port } = server.address() as { port: number };
		const url = `http://127.0.0.1:${port}/`;
		saveCustomProvider({ name: 'Local', api: 'openai', url, key: null });
		saveEmbeddingSetting(parseEmbeddingSetting('custom-openai/local/embeddinggemma:300m'));
		expect(embeddingSource()).toEqual({
			url: `http://127.0.0.1:${port}/v1`,
			model: 'embeddinggemma:300m',
			key: null,
			name: 'Local/embeddinggemma:300m'
		});
		const profile = family();
		await embedMemory(profile.slug);
		expect(requests.map((r) => r.model)).toEqual(['embeddinggemma:300m']);
		const teeth = await recallFor(profile.slug, 'Who fixes our teeth?', { known: '' });
		expect(teeth).toContain('Dentist: Dr. Keller');

		saveCustomProvider({ id: 'local', name: 'Local', url: `${url}v1`, key: 'sk-local' });
		expect(embeddingSource()?.key).toBe('sk-local');

		removeCustomProvider('local');
		expect(embeddingSource()).toBeNull();
		expect(embeddingStatus()).toContain('there is no custom provider "local"');
	});

	it('tells Models & keys what is set and in use, never a key', () => {
		initConfig();
		expect(embeddingState()).toEqual({
			mode: 'auto',
			model: null,
			using: 'openai/text-embedding-3-small'
		});
		saveCustomProvider({
			name: 'Studio',
			api: 'openai',
			url: 'http://localhost:1234',
			key: 'secret'
		});
		saveEmbeddingSetting({ provider: 'custom-openai', model: 'studio/nomic' });
		const state = embeddingState();
		expect(state).toEqual({
			mode: 'custom-openai',
			model: 'studio/nomic',
			using: 'Studio/nomic'
		});
		expect(JSON.stringify(state)).not.toContain('secret');

		vi.stubEnv('OPENAI_API_KEY', '');
		saveEmbeddingSetting({ provider: 'openai', model: 'text-embedding-3-large' });
		expect(embeddingState()).toMatchObject({ mode: 'openai', using: null });
		saveEmbeddingSetting('off');
		expect(embeddingState()).toMatchObject({ mode: 'off', using: null });
		saveEmbeddingSetting(undefined);
		expect(embeddingState().mode).toBe('auto');
	});

	it('reads what `btw config set embeddings` is given', () => {
		expect(parseEmbeddingSetting('auto')).toBeUndefined();
		expect(parseEmbeddingSetting('openrouter/qwen/qwen3-embedding-8b')).toEqual({
			provider: 'openrouter',
			model: 'qwen/qwen3-embedding-8b'
		});
		initConfig();
		saveCustomProvider({ name: 'GPU', api: 'openai', url: 'http://gpu:8000/v1', key: null });
		saveCustomProvider({ name: 'oMLX', api: 'anthropic', url: 'http://localhost:8000', key: null });
		expect(parseEmbeddingSetting('custom-openai/gpu/Qwen/Qwen3-Embedding-8B')).toEqual({
			provider: 'custom-openai',
			model: 'gpu/Qwen/Qwen3-Embedding-8B'
		});
		expect(() => parseEmbeddingSetting('custom-openai/nomic')).toThrow(
			'give the custom provider too'
		);
		expect(() => parseEmbeddingSetting('custom-openai/studio/nomic')).toThrow(
			'no custom provider "studio"'
		);
		expect(() => parseEmbeddingSetting('custom-openai/omlx/nomic')).toThrow('no embeddings');
		expect(() => parseEmbeddingSetting('http://localhost:1234/v1')).toThrow('btw provider add');
		expect(() => parseEmbeddingSetting('voyage/voyage-3')).toThrow('openai/<model>');
		expect(() => parseEmbeddingSetting('openai/')).toThrow('openai/<model>');
	});
});
