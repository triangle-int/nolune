import { readFileSync, statSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import {
	cancelChatGptSignIn,
	chatGptAccount,
	chatGptCredentials,
	chatGptPlanStatus,
	signOutChatGpt,
	startChatGptSignIn
} from './chatgpt-sign-in.ts';
import { streamResponse } from './chatgpt-plan.ts';
import { committedRows, createConversation, getConversation } from './conversations.ts';
import { describeApiError } from './models.ts';
import { paths } from './paths.ts';
import { PlanError } from './plans.ts';
import { addPreset } from './presets.ts';
import { TOOLS } from './run-command.ts';
import { onLoopEnd, sendMessage } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

// --- a stand-in for auth.openai.com (/auth) and ChatGPT's Codex backend (/codex) ---

interface Seen {
	method: string;
	path: string;
	headers: IncomingHttpHeaders;
	body: string;
	json: Record<string, unknown> | null;
}
type Answer = {
	status?: number;
	headers?: Record<string, string>;
	json?: unknown;
	events?: object[];
};

let server: Server;
let base = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;

beforeAll(async () => {
	server = createServer(async (req, res) => {
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk as Buffer);
		const body = Buffer.concat(chunks).toString('utf8');
		let json: Seen['json'] = null;
		try {
			json = JSON.parse(body) as Seen['json'];
		} catch {
			// a form, or empty
		}
		const request: Seen = {
			method: req.method ?? 'GET',
			path: req.url ?? '',
			headers: req.headers,
			body,
			json
		};
		seen.push(request);
		const reply = answer(request);
		if (reply.events) {
			res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream', ...reply.headers });
			for (const e of reply.events) {
				res.write(`event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`);
			}
			res.end();
		} else {
			res.writeHead(reply.status ?? 200, { 'content-type': 'application/json', ...reply.headers });
			res.end(JSON.stringify(reply.json ?? {}));
		}
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	vi.stubEnv('BTW_CHATGPT_ISSUER', `${base}/auth`);
	vi.stubEnv('BTW_CHATGPT_BASE_URL', `${base}/codex`);
	seen.length = 0;
	answer = () => ({ status: 404, json: { error: { message: 'Not found' } } });
});

afterEach(() => {
	cancelChatGptSignIn();
	vi.useRealTimers();
	vi.unstubAllEnvs();
});

// --- tokens ---

const CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann';
const AUTH = 'https://api.openai.com/auth';

function jwt(claims: object): string {
	const part = (value: object) => Buffer.from(JSON.stringify(value)).toString('base64url');
	return `${part({ alg: 'RS256' })}.${part(claims)}.signature`;
}

/** What ChatGPT hands out, the `n`th time. The access token lasts `seconds`. */
function tokens(n: number, seconds = 3600) {
	return {
		id_token: jwt({
			email: 'anna@example.com',
			[AUTH]: { chatgpt_account_id: 'acct-1', chatgpt_plan_type: 'plus' }
		}),
		access_token: jwt({ n, exp: Math.floor(Date.now() / 1000) + seconds }),
		refresh_token: `refresh-${n}`
	};
}

/** Signed in as another btw process would have left it. */
function signedIn(n = 1, seconds = 3600): ReturnType<typeof tokens> {
	const t = tokens(n, seconds);
	writeFileSync(
		paths.chatgptAuth,
		JSON.stringify({
			idToken: t.id_token,
			accessToken: t.access_token,
			refreshToken: t.refresh_token,
			accountId: 'acct-1',
			refreshedAt: new Date().toISOString()
		})
	);
	return t;
}

function stored(): { accessToken: string; refreshToken: string } {
	return JSON.parse(readFileSync(paths.chatgptAuth, 'utf8')) as {
		accessToken: string;
		refreshToken: string;
	};
}

// --- replies ---

function said(text: string) {
	return {
		id: 'msg_1',
		type: 'message',
		role: 'assistant',
		status: 'completed',
		content: [{ type: 'output_text', text, annotations: [] }]
	};
}

function streamed(output: Record<string, unknown>[]): Answer {
	const events: object[] = [{ type: 'response.created', response: { status: 'in_progress' } }];
	output.forEach((item, index) => {
		events.push({ type: 'response.output_item.added', output_index: index, item });
		for (const part of (item.content ?? []) as { text: string }[]) {
			events.push({ type: 'response.output_text.delta', output_index: index, delta: part.text });
		}
		events.push({ type: 'response.output_item.done', output_index: index, item });
	});
	events.push({
		type: 'response.completed',
		response: {
			status: 'completed',
			output,
			usage: { input_tokens: 900, input_tokens_details: { cached_tokens: 600 }, output_tokens: 20 }
		}
	});
	return { events };
}

const turns = () => seen.filter((r) => r.path === '/codex/responses');

const turn = () =>
	streamResponse({
		model: 'gpt-6-astra',
		effort: 'high',
		system: 'You are btw.',
		tools: TOOLS,
		messages: [{ role: 'user', content: [{ type: 'text', text: 'Anna: Hi' }] }],
		cacheKey: 'chat-1',
		signal: new AbortController().signal,
		onEvent: () => {}
	});

function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

/** A 1x1 PNG. */
const PNG = Buffer.from(
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==',
	'base64'
);

// --- tests ---

describe('signing in with ChatGPT', () => {
	it('signs in with a code entered on another device, and keeps the tokens to this user', async () => {
		let polls = 0;
		answer = (req) => {
			if (req.path === '/auth/api/accounts/deviceauth/usercode') {
				return { json: { device_auth_id: 'dev-1', user_code: 'ABCD-1234', interval: '1' } };
			}
			if (req.path === '/auth/api/accounts/deviceauth/token') {
				// Not entered yet, then entered.
				return polls++ === 0
					? { status: 403, json: { error: { message: 'pending' } } }
					: {
							json: {
								authorization_code: 'code-1',
								code_challenge: 'challenge-1',
								code_verifier: 'verifier-1'
							}
						};
			}
			if (req.path === '/auth/oauth/token') return { json: tokens(1) };
			return { status: 404, json: {} };
		};

		const signIn = await startChatGptSignIn();
		expect(signIn).toMatchObject({
			verificationUrl: `${base}/auth/codex/device`,
			userCode: 'ABCD-1234'
		});
		expect(chatGptPlanStatus()).toMatchObject({
			account: null,
			signedIn: null,
			pending: { userCode: 'ABCD-1234', verificationUrl: `${base}/auth/codex/device` }
		});

		expect(await signIn.done).toEqual({ email: 'anna@example.com', plan: 'ChatGPT Plus' });
		// Said as the Claude plan says it.
		expect(chatGptPlanStatus()).toEqual({
			account: { email: 'anna@example.com', plan: 'ChatGPT Plus' },
			signedIn: 'signed in as anna@example.com (ChatGPT Plus)',
			problem: null,
			pending: null,
			signInError: null
		});

		expect(seen[0].json).toEqual({ client_id: CLIENT_ID });
		const polled = seen.filter((r) => r.path === '/auth/api/accounts/deviceauth/token');
		expect(polled.map((r) => r.json)).toEqual([
			{ device_auth_id: 'dev-1', user_code: 'ABCD-1234' },
			{ device_auth_id: 'dev-1', user_code: 'ABCD-1234' }
		]);
		const exchange = seen.find((r) => r.path === '/auth/oauth/token')!;
		expect(Object.fromEntries(new URLSearchParams(exchange.body))).toEqual({
			grant_type: 'authorization_code',
			client_id: CLIENT_ID,
			code: 'code-1',
			redirect_uri: `${base}/auth/deviceauth/callback`,
			code_verifier: 'verifier-1'
		});
		expect(stored()).toMatchObject({ refreshToken: 'refresh-1', accountId: 'acct-1' });
		expect(statSync(paths.chatgptAuth).mode & 0o777).toBe(0o600);
	});

	it('stops waiting when the sign-in is cancelled or its code expires', async () => {
		answer = (req) =>
			req.path === '/auth/api/accounts/deviceauth/usercode'
				? { json: { device_auth_id: 'dev-1', user_code: 'ABCD-1234', interval: '1' } }
				: { status: 403, json: {} };

		const cancelled = await startChatGptSignIn();
		cancelChatGptSignIn();
		await expect(cancelled.done).rejects.toThrow('The sign-in was cancelled.');
		expect(chatGptPlanStatus()).toMatchObject({
			pending: null,
			problem: expect.stringMatching(/^Not signed in with ChatGPT\./),
			signInError: null
		});

		const expiring = await startChatGptSignIn();
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(expiring.expiresAt);
		await expect(expiring.done).rejects.toThrow('The code expired before it was entered.');
		expect(chatGptPlanStatus()).toMatchObject({
			account: null,
			pending: null,
			signInError: 'The code expired before it was entered. Start the sign-in again.'
		});
	});

	it('signs out, asking ChatGPT to revoke the sign-in', async () => {
		signedIn();
		answer = () => ({ json: {} });
		await signOutChatGpt();
		expect(chatGptAccount()).toBeNull();
		expect(seen.map((r) => [r.path, r.json])).toEqual([
			[
				'/auth/oauth/revoke',
				{ token: 'refresh-1', token_type_hint: 'refresh_token', client_id: CLIENT_ID }
			]
		]);
		await expect(chatGptCredentials()).rejects.toThrow('Not signed in with ChatGPT.');
	});
});

describe("the sign-in's tokens", () => {
	it('renews the access token before it runs out, once for requests at the same time', async () => {
		signedIn(1, 60);
		const next = tokens(2);
		answer = (req) => (req.path === '/auth/oauth/token' ? { json: next } : { status: 404 });

		const [a, b] = await Promise.all([chatGptCredentials(), chatGptCredentials()]);
		expect(a).toEqual({ accessToken: next.access_token, accountId: 'acct-1' });
		expect(b).toEqual(a);
		expect(seen.map((r) => r.json)).toEqual([
			{ client_id: CLIENT_ID, grant_type: 'refresh_token', refresh_token: 'refresh-1' }
		]);
		expect(stored()).toMatchObject({ accessToken: next.access_token, refreshToken: 'refresh-2' });

		// Fresh now, so nothing is asked.
		await chatGptCredentials();
		expect(seen).toHaveLength(1);
	});

	it('uses what another btw process renewed, since a refresh token works only once', async () => {
		signedIn(1, 60);
		let theirs: ReturnType<typeof tokens> | undefined;
		answer = () => {
			// The CLI refreshed a moment earlier, with the same refresh token.
			theirs = signedIn(9);
			return { status: 400, json: { error: { code: 'refresh_token_reused', message: 'Reused' } } };
		};
		expect(await chatGptCredentials()).toEqual({
			accessToken: theirs!.access_token,
			accountId: 'acct-1'
		});
	});

	it("keeps using a token that hasn't run out yet when ChatGPT can't renew it", async () => {
		const { access_token } = signedIn(1, 60);
		answer = () => ({ status: 503, json: { error: { message: 'Down for a moment' } } });
		expect(await chatGptCredentials()).toEqual({ accessToken: access_token, accountId: 'acct-1' });

		// Not the one the backend just turned down, though.
		await expect(chatGptCredentials(access_token)).rejects.toThrow(
			"ChatGPT couldn't renew its sign-in (503: Down for a moment). Try again in a moment."
		);
	});

	it('asks for a new sign-in once ChatGPT has ended this one', async () => {
		signedIn(1, -60);
		answer = () => ({ status: 401, json: { error: { code: 'refresh_token_expired' } } });
		const err = await chatGptCredentials().catch((e: unknown) => e);
		expect(err).toBeInstanceOf(PlanError);
		expect((err as Error).message).toBe(
			'The ChatGPT sign-in has expired or was signed out. An admin can sign in under Models & keys in btw, or with `btw chatgpt-plan setup`.'
		);
	});
});

describe('a chat on a ChatGPT plan', () => {
	function planChat() {
		const { user, profile } = makeFamily();
		const preset = makePreset('ChatGPT', 'gpt-6-astra', 'chatgpt-plan');
		return {
			user,
			profile,
			chat: createConversation({ profile, presetId: preset.id, userId: user.id })
		};
	}

	it("runs on Codex's backend with the sign-in's token, and names the chat there too", async () => {
		const { access_token } = signedIn();
		const { user, chat } = planChat();
		answer = (req) => {
			if (req.path !== '/codex/responses') return { status: 404, json: {} };
			return (req.json?.input as unknown[]).length === 1 && !req.json?.tools
				? streamed([said('Saying hello')])
				: streamed([said('Hi Anna.')]);
		};

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Hi');
		await ended;

		const main = turns().find((r) => r.json?.tools)!;
		expect(main.headers).toMatchObject({
			authorization: `Bearer ${access_token}`,
			'chatgpt-account-id': 'acct-1',
			originator: 'btw',
			'session-id': chat.id
		});
		expect(main.json).toMatchObject({
			model: 'gpt-6-astra',
			instructions: chat.systemPrompt,
			store: false,
			stream: true,
			prompt_cache_key: chat.id,
			reasoning: { effort: 'medium', summary: 'auto' },
			include: ['reasoning.encrypted_content'],
			input: [{ role: 'user', content: [{ type: 'input_text', text: 'Anna: Hi' }] }]
		});
		const saved = committedRows(chat.id).find((row) => row.kind === 'assistant')!;
		expect(JSON.parse(saved.content)).toEqual([said('Hi Anna.')]);

		// The title: streamed too, and without max_output_tokens, which the backend refuses.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Saying hello'));
		const naming = turns().find((r) => !r.json?.tools)!.json!;
		expect(naming).toMatchObject({ stream: true, store: false, reasoning: { effort: 'low' } });
		expect(naming).not.toHaveProperty('max_output_tokens');
	});

	it('sends pictures inline and PDFs as their path, having no Files API', async () => {
		signedIn();
		const { user, profile, chat } = planChat();
		const photo = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'dot.png',
			body: Readable.from([PNG])
		});
		const pdf = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'menu.pdf',
			body: Readable.from([Buffer.from('%PDF-1.4\n% a menu\n')])
		});
		answer = (req) =>
			req.path === '/codex/responses' ? streamed([said('A dot.')]) : { status: 404 };

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What are these?', [photo.id, pdf.id]);
		await ended;

		expect(seen.every((r) => r.path === '/codex/responses')).toBe(true);
		const input = turns().find((r) => r.json?.tools)!.json!.input as {
			content: { type: string; text?: string; image_url?: string }[];
		}[];
		expect(input[0].content).toEqual([
			{ type: 'input_text', text: expect.stringMatching(/^\[Anna attached dot\.png, saved at /) },
			{
				type: 'input_image',
				image_url: `data:image/png;base64,${PNG.toString('base64')}`,
				detail: 'auto'
			},
			{
				type: 'input_text',
				text: expect.stringMatching(
					/^\[Anna attached menu\.pdf, saved at .*It isn't shown here: models on the ChatGPT plan don't take PDFs\]$/
				)
			},
			{ type: 'input_text', text: 'Anna: What are these?' }
		]);
		const human = committedRows(chat.id).find((row) => row.kind === 'human')!;
		expect(JSON.parse(human.attachments!)).toMatchObject([
			{ sentAs: 'image' },
			{ sentAs: 'path', note: "models on the ChatGPT plan don't take PDFs" }
		]);
	});

	it('renews the sign-in and tries once more when the backend turns the token down', async () => {
		signedIn(1);
		const next = tokens(2);
		answer = (req) => {
			if (req.path === '/auth/oauth/token') return { json: next };
			return req.headers.authorization === `Bearer ${next.access_token}`
				? streamed([said('Hello.')])
				: { status: 401, json: { error: { message: 'Token expired' } } };
		};
		expect((await turn()).output).toEqual([said('Hello.')]);
		expect(seen.map((r) => r.path)).toEqual([
			'/codex/responses',
			'/auth/oauth/token',
			'/codex/responses'
		]);
	});

	it("checks the model in the plan's catalog when a preset is added", async () => {
		signedIn();
		answer = (req) =>
			req.path === '/codex/models?client_version=0.157.1'
				? {
						json: {
							models: [
								{ slug: 'gpt-6-astra', display_name: 'GPT-6 Astra', context_window: 272000 },
								{ slug: 'gpt-5.5', display_name: 'GPT-5.5', context_window: 272000 },
								{ slug: 'codex-auto-review', visibility: 'hide', context_window: 272000 }
							]
						}
					}
				: { status: 404 };

		const preset = await addPreset({ provider: 'chatgpt-plan', model: 'gpt-6-astra' });
		expect(preset).toMatchObject({
			name: 'gpt-6-astra (chatgpt-plan)',
			provider: 'chatgpt-plan',
			modelContextWindow: 272000
		});
		await expect(addPreset({ provider: 'chatgpt-plan', model: 'gpt-9' })).rejects.toThrow(
			'Could not verify model "gpt-9": ChatGPT has no model "gpt-9" for Codex. It has gpt-6-astra, gpt-5.5.'
		);
	});

	it("explains the plan's limits and a missing sign-in in plain words", async () => {
		signedIn();
		const resetsAt = Math.floor(Date.now() / 1000) + 2 * 3600;
		answer = () => ({
			status: 429,
			headers: { 'retry-after-ms': '1' },
			json: { error: { type: 'usage_limit_reached', resets_at: resetsAt, plan_type: 'plus' } }
		});
		const limited = await turn().catch((err: unknown) => err);
		// The same kind of error as the Claude plan's.
		expect(limited).toBeInstanceOf(PlanError);
		expect((limited as PlanError).kind).toBe('usage_limit_reached');
		expect(describeApiError(limited)).toBe(
			"The ChatGPT plan's Codex limit is used up for now. It resets in about 2 hours."
		);

		answer = () => ({
			status: 429,
			headers: { 'retry-after-ms': '1' },
			json: { error: { type: 'usage_not_included' } }
		});
		const notIncluded = await turn().catch((err: unknown) => err);
		expect(describeApiError(notIncluded)).toBe("This ChatGPT plan doesn't include Codex.");

		await signOutChatGpt();
		const missing = await turn().catch((err: unknown) => err);
		expect(describeApiError(missing)).toBe(
			'Not signed in with ChatGPT. An admin can sign in under Models & keys in btw, or with `btw chatgpt-plan setup`.'
		);
	});
});
