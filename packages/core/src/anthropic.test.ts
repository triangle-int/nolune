import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { listModels } from './anthropic.ts';

// --- a stand-in for Anthropic's models API, two models to a page ---

const MODELS = [
	{ id: 'claude-fable-5-1', display_name: 'Claude Fable 5.1', max_input_tokens: 1_000_000 },
	{ id: 'claude-opus-5-5', display_name: 'Claude Opus 5.5', max_input_tokens: 1_000_000 },
	{ id: 'claude-sonnet-5', display_name: 'Claude Sonnet 5', max_input_tokens: 1_000_000 },
	{ id: 'claude-haiku-4-5-20251001', display_name: 'Claude Haiku 4.5', max_input_tokens: null }
];
let server: Server;
const seen: string[] = [];

beforeAll(async () => {
	server = createServer((req, res) => {
		seen.push(req.url ?? '');
		const url = new URL(req.url ?? '', 'http://localhost');
		const after = url.searchParams.get('after_id');
		const start = after ? MODELS.findIndex((m) => m.id === after) + 1 : 0;
		const page = MODELS.slice(start, start + 2).map((m) => ({
			...m,
			type: 'model',
			created_at: '2026-01-01T00:00:00Z',
			capabilities: null,
			max_tokens: 64_000
		}));
		res.writeHead(200, { 'content-type': 'application/json' });
		res.end(
			JSON.stringify({
				data: page,
				has_more: start + 2 < MODELS.length,
				first_id: page[0]?.id ?? null,
				last_id: page.at(-1)?.id ?? null
			})
		);
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	vi.stubEnv('ANTHROPIC_BASE_URL', `http://127.0.0.1:${(server.address() as AddressInfo).port}`);
	vi.stubEnv('ANTHROPIC_API_KEY', 'sk-ant-test-0000000000000001');
	seen.length = 0;
});

describe("Anthropic's models", () => {
	it('lists every page, with the names and windows Anthropic gives', async () => {
		expect(await listModels()).toEqual([
			{
				id: 'claude-fable-5-1',
				name: 'Claude Fable 5.1',
				description: null,
				contextWindow: 1_000_000
			},
			{
				id: 'claude-opus-5-5',
				name: 'Claude Opus 5.5',
				description: null,
				contextWindow: 1_000_000
			},
			{
				id: 'claude-sonnet-5',
				name: 'Claude Sonnet 5',
				description: null,
				contextWindow: 1_000_000
			},
			{
				id: 'claude-haiku-4-5-20251001',
				name: 'Claude Haiku 4.5',
				description: null,
				contextWindow: null
			}
		]);
		expect(seen).toHaveLength(2);
	});
});
