import { describe, expect, it } from 'vitest';
import { readAccount, updateAccount } from './accounts.ts';
import { addExtra, endPlan, HOUR, startPlan, type PeriodGrant } from './limits.ts';
import { chatModels, OpenRouter } from './openrouter.ts';
import { MAX_TOKENS, Proxy, type Endpoint } from './proxy.ts';
import { addUser, testDb } from './test/db.ts';

const t0 = Date.UTC(2026, 9, 1, 18, 0);
const DAY = 24 * HOUR;
const family: PeriodGrant = {
	source: 'in_1',
	credits: 25_000_000,
	carryOver: 12_500_000,
	limits: { window: 1_800_000, week: 8_750_000 },
	renewsAt: t0 + 30 * 24 * HOUR
};
const MODELS = {
	data: [
		{
			id: 'anthropic/claude-haiku-4.5',
			pricing: { prompt: '0.000001', input_cache_read: '0.0000001' },
			supported_parameters: ['tools', 'reasoning']
		},
		{ id: 'some/model-without-tools', supported_parameters: ['temperature'] },
		// OpenRouter lists a free variant, and some variants with no model of their own, as models.
		{ id: 'anthropic/claude-haiku-4.5:free', supported_parameters: ['tools'] },
		{ id: 'apodex/apodex-1.1-mini:free', supported_parameters: ['tools'] },
		{ id: 'someone/thinker:thinking', supported_parameters: ['tools'] }
	]
};
const IMAGE_MODELS = {
	data: [
		{
			id: 'openai/gpt-image-2.5-flare',
			architecture: { input_modalities: ['text', 'image'], output_modalities: ['image'] },
			supported_parameters: { n: { type: 'range', min: 1, max: 10 } }
		}
	]
};
const encoder = new TextEncoder();

interface Call {
	url: string;
	body: string;
	headers: Headers;
	signal?: AbortSignal | null;
}

/** OpenRouter as a function of the request's path, and every call it got. */
async function setup(
	reply: (path: string, call: Call) => Response | Promise<Response>,
	withPlan = true
) {
	const calls: Call[] = [];
	const openrouter = new OpenRouter({
		apiKey: 'sk-or-nolune',
		baseURL: 'https://openrouter.test/api/v1',
		fetch: async (input, init) => {
			const url = String(input);
			const call = {
				url,
				body: String(init?.body ?? ''),
				headers: new Headers(init?.headers),
				signal: init?.signal
			};
			calls.push(call);
			const path = new URL(url).pathname.replace('/api/v1', '');
			if (path === '/models') return Response.json(MODELS);
			if (path === '/images/models') return Response.json(IMAGE_MODELS);
			return reply(path, call);
		}
	});
	const db = await testDb();
	const userId = await addUser(db);
	if (withPlan) await updateAccount(db, userId, () => startPlan(family, t0));
	const logged: unknown[][] = [];
	const log = {
		warn: (...args: unknown[]) => logged.push(args),
		error: (...args: unknown[]) => logged.push(args)
	};
	const proxy = new Proxy({ db, openrouter, now: () => t0 + HOUR, settleAfter: [0], log });
	const send = (body: unknown, endpoint: Endpoint = 'chat', headers: Record<string, string> = {}) =>
		proxy.handle({
			userId,
			endpoint,
			headers: new Headers(headers),
			body: typeof body === 'string' ? body : JSON.stringify(body)
		});
	const forwarded = () => calls.filter((call) => !call.url.endsWith('/models'));
	const sendImage = (body: unknown, signal?: AbortSignal) =>
		proxy.handle({
			userId,
			endpoint: 'image',
			headers: new Headers(),
			body: JSON.stringify(body),
			signal
		});
	return { db, userId, proxy, openrouter, send, sendImage, calls: forwarded, logged };
}

const chat = { model: 'anthropic/claude-haiku-4.5', messages: [{ role: 'user', content: 'Hi' }] };
const usage = { prompt_tokens: 10, completion_tokens: 5, cost: 0.001 };

function sse(...chunks: unknown[]): string {
	return chunks
		.map((chunk) => `data: ${typeof chunk === 'string' ? chunk : JSON.stringify(chunk)}\n\n`)
		.join('');
}

describe('nolune’s API in front of OpenRouter', () => {
	it('passes a reply on as it came, and charges what it cost the plan', async () => {
		const reply = `{"id":"gen-1","choices":[{"message":{"content":"Hi!"}}],"usage":${JSON.stringify(
			{
				...usage,
				is_byok: true,
				cost_details: { upstream_inference_cost: 0.002 }
			}
		)}}`;
		const s = await setup(
			() => new Response(reply, { headers: { 'content-type': 'application/json' } })
		);
		const response = await s.send(chat);

		expect(response.status).toBe(200);
		expect(await response.text()).toBe(reply);
		const [call] = s.calls();
		expect(call.headers.get('authorization')).toBe('Bearer sk-or-nolune');
		expect(JSON.parse(call.body)).toEqual({ ...chat, max_tokens: MAX_TOKENS });

		// OpenRouter's $0.001, and the $0.002 the provider billed nolune's own key there.
		const account = await readAccount(s.db, s.userId);
		expect(account?.window?.spent).toBe(3000);
		expect(account?.credits[0].left).toBe(25_000_000 - 3000);
		expect(JSON.parse(response.headers.get('x-nolune-usage')!).window).toMatchObject({
			spent: 3000
		});
	});

	it('leaves a body that asks for little enough as it is', async () => {
		const s = await setup(() => Response.json({ id: 'gen-1', usage }));
		const body = JSON.stringify({
			...chat,
			max_tokens: 1000,
			cache_control: { type: 'ephemeral' }
		});
		await s.send(body);
		expect(s.calls()[0].body).toBe(body);
	});

	it('streams a reply on, and charges what its last chunk says', async () => {
		const stream = sse(
			{ id: 'gen-2', choices: [{ delta: { content: 'Hi' } }] },
			{ id: 'gen-2', choices: [{ delta: {}, finish_reason: 'stop' }], usage },
			'[DONE]'
		);
		const s = await setup(
			() => new Response(stream, { headers: { 'content-type': 'text/event-stream' } })
		);
		const response = await s.send({ ...chat, stream: true });

		expect(response.headers.get('content-type')).toBe('text/event-stream');
		expect(await response.text()).toBe(stream);
		await s.proxy.settled();
		expect((await readAccount(s.db, s.userId))?.window?.spent).toBe(1000);
	});

	it('asks OpenRouter later what a stream that was cut off cost', async () => {
		const s = await setup((path) => {
			if (path === '/generation') {
				return Response.json({ data: { total_cost: 0.0004, is_byok: false } });
			}
			const body = new ReadableStream<Uint8Array>({
				start: (controller) =>
					controller.enqueue(
						encoder.encode(sse({ id: 'gen-3', choices: [{ delta: { content: 'Hi' } }] }))
					)
			});
			return new Response(body, { headers: { 'content-type': 'text/event-stream' } });
		});
		const response = await s.send({ ...chat, stream: true });
		const reader = response.body!.getReader();
		await reader.read();
		await reader.cancel();

		await s.proxy.settled();
		expect(s.calls().at(-1)?.url).toBe('https://openrouter.test/api/v1/generation?id=gen-3');
		expect((await readAccount(s.db, s.userId))?.window?.spent).toBe(400);
	});

	it('refuses past a limit without asking OpenRouter, and says when it starts again', async () => {
		const s = await setup(() => Response.json({ id: 'gen-1', usage }));
		await updateAccount(s.db, s.userId, (account) => ({
			...account!,
			window: { openedAt: t0, spent: 1_800_000 }
		}));
		const response = await s.send(chat);

		expect(response.status).toBe(429);
		expect(response.headers.get('x-should-retry')).toBe('false');
		expect(response.headers.get('retry-after')).toBe(String(4 * 60 * 60));
		expect(await response.json()).toEqual({
			error: {
				code: 'five_hour_limit',
				message: "nolune's 5-hour limit is reached.",
				resets_at: new Date(t0 + 5 * HOUR).toISOString()
			}
		});
		expect(s.calls()).toHaveLength(0);
	});

	it('lets a turn that is going finish past a limit', async () => {
		const s = await setup(() => Response.json({ id: 'gen-1', usage }));
		await updateAccount(s.db, s.userId, (account) => ({
			...account!,
			window: { openedAt: t0, spent: 1_800_000 }
		}));
		const response = await s.send(chat, 'chat', { 'x-nolune-turn': 'continue' });
		expect(response.status).toBe(200);
	});

	it('counts embeddings against the month only', async () => {
		const s = await setup(() =>
			Response.json({ id: 'gen-emb-1', data: [], usage: { cost: 0.00002 } })
		);
		await updateAccount(s.db, s.userId, (account) => ({
			...account!,
			window: { openedAt: t0, spent: 1_800_000 }
		}));
		const response = await s.send(
			{ model: 'openai/text-embedding-3-small', input: ['milk'] },
			'embedding',
			{ 'x-nolune-use': 'background' }
		);

		expect(response.status).toBe(200);
		expect(s.calls()[0].url).toBe('https://openrouter.test/api/v1/embeddings');
		const account = await readAccount(s.db, s.userId);
		expect(account?.window?.spent).toBe(1_800_000);
		expect(account?.credits[0].left).toBe(25_000_000 - 20);
	});

	it('refuses an account with no plan, and models the plan doesn’t offer', async () => {
		const none = await setup(() => Response.json({}), false);
		expect((await none.send(chat)).status).toBe(402);

		// A plan that ended keeps its pack for the next one, and spends nothing until then.
		const ended = await setup(() => Response.json({}));
		await updateAccount(ended.db, ended.userId, (account) => ({
			...endPlan(
				addExtra(account!, { source: 'cs_1', credits: 10_000_000, expiresAt: t0 + 365 * DAY })
			),
			extraPastLimits: true
		}));
		const refused = await ended.send(chat);
		expect(refused.status).toBe(402);
		expect((await refused.json()).error.code).toBe('no_plan');
		expect(ended.calls()).toHaveLength(0);

		const s = await setup(() => Response.json({}));
		const noTools = await s.send({ ...chat, model: 'some/model-without-tools' });
		expect(noTools.status).toBe(400);
		expect((await noTools.json()).error.code).toBe('model_not_offered');
		const embedding = await s.send({ model: 'someone/else', input: 'x' }, 'embedding');
		expect(embedding.status).toBe(400);
		// Free models, listed on their own or as a variant of one the plan offers.
		for (const model of ['apodex/apodex-1.1-mini:free', 'anthropic/claude-haiku-4.5:free']) {
			const free = await s.send({ ...chat, model });
			expect(free.status).toBe(400);
			expect((await free.json()).error.code).toBe('model_not_offered');
		}
		expect(s.calls()).toHaveLength(0);
	});

	it('offers what it lists: models that call tools, with variants and no free ones', async () => {
		const s = await setup(() => Response.json({ choices: [], usage }));
		const listed = chatModels(await s.openrouter.models()).map((m) => m.id);
		expect(listed).toEqual(['anthropic/claude-haiku-4.5', 'someone/thinker:thinking']);
		for (const model of [...listed, 'anthropic/claude-haiku-4.5:nitro']) {
			expect((await s.send({ ...chat, model })).status).toBe(200);
		}
	});

	it('passes a picture request on to the Image API, and charges what it cost', async () => {
		const picture = { model: 'openai/gpt-image-2.5-flare', prompt: 'a paper boat', n: 2 };
		const reply = JSON.stringify({
			created: 0,
			data: [{ b64_json: 'iVBORw0KGgo=', media_type: 'image/png' }],
			usage: { prompt_tokens: 12, completion_tokens: 6144, cost: 0.08, is_byok: false }
		});
		const s = await setup(
			() => new Response(reply, { headers: { 'content-type': 'application/json' } })
		);
		const response = await s.sendImage(picture);

		expect(response.status).toBe(200);
		expect(await response.text()).toBe(reply);
		const [call] = s.calls();
		expect(call.url).toBe('https://openrouter.test/api/v1/images');
		expect(JSON.parse(call.body)).toEqual(picture);
		const account = await readAccount(s.db, s.userId);
		// Against the 5-hour and weekly limits as much as a chat.
		expect(account).toMatchObject({ window: { spent: 80_000 }, week: { spent: 80_000 } });
	});

	it('charges a picture by its generation when the reply says no cost', async () => {
		const s = await setup((path) => {
			if (path === '/generation') return Response.json({ data: { total_cost: 0.04 } });
			return Response.json(
				{ created: 0, data: [{ b64_json: 'iVBORw0KGgo=' }] },
				{ headers: { 'x-generation-id': 'gen-img-1' } }
			);
		});
		await s.sendImage({ model: 'openai/gpt-image-2.5-flare', prompt: 'a paper boat' });
		await s.proxy.settled();
		expect(s.calls().at(-1)?.url).toBe('https://openrouter.test/api/v1/generation?id=gen-img-1');
		expect((await readAccount(s.db, s.userId))?.window?.spent).toBe(40_000);
	});

	it('finishes and charges a picture whoever asked stopped waiting for', async () => {
		let upstreamSignal: AbortSignal | null | undefined;
		const gone = new AbortController();
		const s = await setup(async (_path, call) => {
			upstreamSignal = call.signal;
			gone.abort();
			return Response.json({ created: 0, data: [], usage: { cost: 0.05 } });
		});
		await s.sendImage({ model: 'openai/gpt-image-2.5-flare', prompt: 'a paper boat' }, gone.signal);
		expect(upstreamSignal ?? undefined).toBeUndefined();
		expect((await readAccount(s.db, s.userId))?.window?.spent).toBe(50_000);
	});

	it('refuses pictures from models it doesn’t offer, too many at once, and streams', async () => {
		const s = await setup(() => Response.json({}));
		const other = await s.sendImage({ model: 'someone/else', prompt: 'x' });
		expect(other.status).toBe(400);
		expect((await other.json()).error.code).toBe('model_not_offered');
		const many = await s.sendImage({ model: 'openai/gpt-image-2.5-flare', prompt: 'x', n: 5 });
		expect((await many.json()).error.code).toBe('too_many_images');
		const streamed = await s.sendImage({
			model: 'openai/gpt-image-2.5-flare',
			prompt: 'x',
			stream: true
		});
		expect(streamed.status).toBe(400);
		expect(s.calls()).toHaveLength(0);
	});

	it('keeps trouble with nolune’s own key from the family, and passes other errors on', async () => {
		const broke = await setup(() =>
			Response.json({ error: { message: 'Insufficient credits', code: 402 } }, { status: 402 })
		);
		const response = await broke.send(chat);
		expect(response.status).toBe(503);
		expect((await response.json()).error.code).toBe('upstream_unavailable');
		expect(broke.logged).toHaveLength(1);

		const flagged = { error: { message: 'Input was flagged', code: 403 } };
		const s = await setup(() => Response.json(flagged, { status: 403 }));
		const passed = await s.send(chat);
		expect(passed.status).toBe(403);
		expect(await passed.json()).toEqual(flagged);
		expect((await readAccount(s.db, s.userId))?.window).toBeNull();
	});
});
