import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { listModels, streamTurn } from './anthropic.ts';
import { summarizeAnthropicUsage, type StreamEvent } from './models.ts';

// --- a stand-in for Anthropic's models API, two models to a page ---

const MODELS = [
	{ id: 'claude-fable-5-1', display_name: 'Claude Fable 5.1', max_input_tokens: 1_000_000 },
	{ id: 'claude-opus-5-5', display_name: 'Claude Opus 5.5', max_input_tokens: 1_000_000 },
	{ id: 'claude-sonnet-5', display_name: 'Claude Sonnet 5', max_input_tokens: 1_000_000 },
	{ id: 'claude-haiku-4-5-20251001', display_name: 'Claude Haiku 4.5', max_input_tokens: null }
];
let server: Server;
const seen: string[] = [];

// --- and for its Messages API, a reply that summarized the conversation first ---

const SUMMARIZED = [
	{
		type: 'message_start',
		message: {
			id: 'msg_1',
			type: 'message',
			role: 'assistant',
			model: 'claude-opus-5-5',
			content: [],
			stop_reason: null,
			stop_sequence: null,
			usage: { input_tokens: 0, output_tokens: 0 }
		}
	},
	{
		type: 'content_block_start',
		index: 0,
		content_block: { type: 'compaction', content: null, encrypted_content: null }
	},
	{
		type: 'content_block_delta',
		index: 0,
		delta: {
			type: 'compaction_delta',
			content: 'Anna asked for the files.',
			encrypted_content: 'e'
		}
	},
	{ type: 'content_block_stop', index: 0 },
	{ type: 'content_block_start', index: 1, content_block: { type: 'text', text: '' } },
	{ type: 'content_block_delta', index: 1, delta: { type: 'text_delta', text: 'Two files.' } },
	{ type: 'content_block_stop', index: 1 },
	{
		type: 'message_delta',
		delta: { stop_reason: 'end_turn', stop_sequence: null },
		usage: {
			input_tokens: 2_000,
			output_tokens: 4,
			cache_read_input_tokens: 0,
			cache_creation_input_tokens: 2_000,
			iterations: [
				{
					type: 'compaction',
					input_tokens: 1_000,
					output_tokens: 900,
					cache_read_input_tokens: 850_000,
					cache_creation_input_tokens: 0
				},
				{
					type: 'message',
					input_tokens: 2_000,
					output_tokens: 4,
					cache_read_input_tokens: 0,
					cache_creation_input_tokens: 2_000
				}
			]
		}
	},
	{ type: 'message_stop' }
];
const requests: { beta: string | undefined; body: Record<string, unknown> }[] = [];

beforeAll(async () => {
	server = createServer((req, res) => {
		seen.push(req.url ?? '');
		const url = new URL(req.url ?? '', 'http://localhost');
		if (req.method === 'POST' && url.pathname === '/v1/messages') {
			let body = '';
			req.on('data', (chunk) => (body += chunk));
			req.on('end', () => {
				requests.push({
					beta: req.headers['anthropic-beta'] as string | undefined,
					body: JSON.parse(body)
				});
				res.writeHead(200, { 'content-type': 'text/event-stream' });
				res.end(SUMMARIZED.map((e) => `event: ${e.type}\ndata: ${JSON.stringify(e)}\n\n`).join(''));
			});
			return;
		}
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
	requests.length = 0;
});

describe('compaction', () => {
	const turn = (
		model: string,
		compactAt: number | null,
		onEvent: (event: StreamEvent) => void = () => {}
	) =>
		streamTurn({
			model,
			effort: 'medium',
			system: 'You are nolune.',
			tools: [],
			cacheTtl: '1h',
			messages: [{ role: 'user', blocks: [{ type: 'text', text: 'Anna: Files?' }] }],
			compactAt,
			signal: new AbortController().signal,
			onEvent
		});

	it('asks Claude to summarize on the server, and streams the summary as a block', async () => {
		const events: StreamEvent[] = [];
		const reply = await turn('claude-opus-5-5', 850_000, (event) => events.push(event));

		expect(requests[0].beta).toBe('compact-2026-01-12');
		expect(requests[0].body.context_management).toEqual({
			edits: [{ type: 'compact_20260112', trigger: { type: 'input_tokens', value: 850_000 } }]
		});
		expect(reply.content).toEqual([
			{ type: 'compaction', content: 'Anna asked for the files.', encrypted_content: 'e' },
			{ type: 'text', text: 'Two files.' }
		]);
		expect(events).toEqual([
			{ type: 'block_start', index: 0, block: { type: 'compaction' } },
			{ type: 'delta', index: 0, text: 'Anna asked for the files.' },
			{ type: 'block_start', index: 1, block: { type: 'text' } },
			{ type: 'delta', index: 1, text: 'Two files.' }
		]);
		expect(summarizeAnthropicUsage(reply.usage)).toEqual({
			input: 2_000,
			cacheRead: 0,
			cacheWrite: 2_000,
			output: 4,
			compaction: { input: 1_000, cacheRead: 850_000, cacheWrite: 0, output: 900 }
		});
	});

	it("leaves it out without a threshold, and for models that can't", async () => {
		await turn('claude-opus-5-5', null);
		await turn('claude-haiku-4-5', 170_000);
		for (const request of requests) {
			expect(request.beta).toBeUndefined();
			expect(request.body).not.toHaveProperty('context_management');
		}
	});
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
