import { readFileSync, statSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import * as keys from './api-keys.ts';
import { configuredApiKey, initConfig, readConfig } from './config.ts';
import { paths } from './paths.ts';

/** Plays both providers: answers by the key it's given. */
const ANSWERS: Record<string, [number, object]> = {
	'sk-ant-good-0000000000001234': [200, { data: [] }],
	'sk-openai-good-000000005678': [200, { data: [] }],
	'sk-restricted-0000000000000': [403, { error: { message: 'Missing scopes: api.model.read' } }],
	'sk-broke-000000000000000000': [429, { error: { message: 'You exceeded your current quota' } }],
	'sk-down-0000000000000000000': [503, { error: { message: 'Overloaded' } }]
};
let server: Server;
const seen: { path: string; key: string | undefined }[] = [];

beforeAll(async () => {
	server = createServer((req, res) => {
		const key = req.headers['x-api-key'] ?? req.headers.authorization?.replace(/^Bearer /, '');
		seen.push({ path: req.url ?? '', key: key as string | undefined });
		const [status, body] = ANSWERS[key as string] ?? [
			401,
			{ error: { message: 'invalid x-api-key' } }
		];
		res.writeHead(status, { 'content-type': 'application/json' });
		res.end(JSON.stringify(body));
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	const { port } = server.address() as AddressInfo;
	vi.stubEnv('ANTHROPIC_BASE_URL', `http://127.0.0.1:${port}`);
	vi.stubEnv('OPENAI_BASE_URL', `http://127.0.0.1:${port}/v1`);
});

afterAll(() => {
	server.close();
	vi.unstubAllEnvs();
});

beforeEach(() => {
	// The test setup empties the btw home before each test: a fresh config.json without keys.
	initConfig();
	vi.stubEnv('ANTHROPIC_API_KEY', '');
	vi.stubEnv('OPENAI_API_KEY', '');
	seen.length = 0;
});

const status = (provider: string) => keys.apiKeyStatuses().find((s) => s.provider === provider)!;

describe('apiKeyStatuses', () => {
	it('says where each key comes from and shows only its end', () => {
		keys.saveApiKey('anthropic', 'sk-ant-good-0000000000001234');
		vi.stubEnv('OPENAI_API_KEY', 'sk-openai-good-000000005678');
		expect(status('anthropic')).toMatchObject({ source: 'config', hint: '1234' });
		expect(status('openai')).toMatchObject({ source: 'env', hint: '5678', envSet: true });
		expect(JSON.stringify(keys.apiKeyStatuses())).not.toContain('good');
		vi.stubEnv('OPENAI_API_KEY', '');
		expect(status('openai')).toMatchObject({ source: null, hint: null, envSet: false });
	});

	it('prefers the saved key to the environment, and falls back to it when removed', () => {
		vi.stubEnv('ANTHROPIC_API_KEY', 'sk-from-the-environment-9999');
		keys.saveApiKey('anthropic', 'sk-ant-good-0000000000001234');
		expect(configuredApiKey('anthropic')).toEqual({
			key: 'sk-ant-good-0000000000001234',
			source: 'config'
		});
		keys.removeApiKey('anthropic');
		expect(readConfig()).not.toHaveProperty('anthropicApiKey');
		expect(configuredApiKey('anthropic')?.source).toBe('env');
	});

	it('keeps config.json private', () => {
		keys.saveApiKey('openai', 'sk-openai-good-000000005678');
		expect(JSON.parse(readFileSync(paths.config, 'utf8')).openaiApiKey).toBe(
			'sk-openai-good-000000005678'
		);
		expect(statSync(paths.config).mode & 0o777).toBe(0o600);
	});
});

describe('normalizeApiKey', () => {
	it('trims what was pasted and refuses what cannot be a key', () => {
		expect(keys.normalizeApiKey('  sk-abc \n')).toBe('sk-abc');
		expect(() => keys.normalizeApiKey('   ')).toThrow('Paste the key first');
		expect(() => keys.normalizeApiKey('sk-abc def')).toThrow('spaces');
	});
});

describe('checkApiKey', () => {
	it('lists models with the key, the way each provider wants it', async () => {
		await expect(keys.checkApiKey('anthropic', 'sk-ant-good-0000000000001234')).resolves.toBe(null);
		await expect(keys.checkApiKey('openai', 'sk-openai-good-000000005678')).resolves.toBe(null);
		expect(seen.map((s) => s.path)).toEqual(['/v1/models?limit=1', '/v1/models']);
	});

	it('refuses a key the provider rejects', async () => {
		await expect(keys.checkApiKey('anthropic', 'sk-ant-wrong')).rejects.toMatchObject({
			reason: 'rejected',
			message: expect.stringContaining("Anthropic didn't accept this key")
		});
	});

	it('takes a working key with a warning when the account has a problem', async () => {
		await expect(keys.checkApiKey('openai', 'sk-restricted-0000000000000')).resolves.toContain(
			'Missing scopes'
		);
		await expect(keys.checkApiKey('openai', 'sk-broke-000000000000000000')).resolves.toContain(
			'quota'
		);
	});

	it('says when it could not check', async () => {
		await expect(
			keys.checkApiKey('anthropic', 'sk-down-0000000000000000000')
		).rejects.toMatchObject({
			reason: 'unchecked'
		});
		vi.stubEnv('OPENAI_BASE_URL', 'http://127.0.0.1:9/v1');
		await expect(keys.checkApiKey('openai', 'sk-openai-good-000000005678')).rejects.toMatchObject({
			reason: 'unchecked',
			message: expect.stringContaining("Couldn't reach OpenAI")
		});
	});
});
