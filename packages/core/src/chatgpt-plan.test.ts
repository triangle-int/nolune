import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import { createRequire } from 'node:module';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import {
	cancelChatGptSignIn,
	chatGptPlanStatus,
	chatGptSignInState,
	listChatGptModels,
	signOutChatGpt,
	startChatGptSignIn
} from './chatgpt-plan.ts';
import { AppServer } from './codex-app-server.ts';
import { initConfig, updateConfig } from './config.ts';
import {
	appendRow,
	committedRows,
	createConversation,
	getConversation,
	insertQueued,
	setEffort,
	setProviderSession
} from './conversations.ts';
import { viewImage } from './images.ts';
import { listModels } from './models.ts';
import { paths } from './paths.ts';
import { addPreset } from './presets.ts';
import { runCommand } from './run-command.ts';
import { getSnapshot, kick, onLoopEnd, sendMessage, stop } from './runner.ts';
import { codexRequests, useFakeCodex } from './test/fake-codex.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

/*
 * Chats run the real Codex, the one the @openai/codex package brings for this platform, against
 * a stand-in for OpenAI's Responses API, set up as Codex's model provider in nolune's Codex home.
 * Nothing reaches OpenAI and no account is used. Signing in with ChatGPT can't be done without
 * OpenAI, so those tests run a stand-in for Codex's app server instead.
 */

/** The Codex that comes with the @openai/codex package, if this platform has one. */
function bundledCodex(): string | null {
	try {
		const require = createRequire(import.meta.url);
		const manifest = require.resolve('@openai/codex/package.json');
		const arch = process.arch === 'x64' ? 'x64' : 'arm64';
		createRequire(manifest).resolve(`@openai/codex-${process.platform}-${arch}/package.json`);
		return join(dirname(manifest), 'bin', 'codex.js');
	} catch {
		return null;
	}
}

const codex = bundledCodex();

// --- a stand-in for OpenAI's Responses API ---

interface Seen {
	path: string;
	headers: IncomingHttpHeaders;
	json: {
		model?: string;
		input?: Record<string, unknown>[];
		reasoning?: { effort?: string; summary?: string };
	} | null;
}
type Item = Record<string, unknown>;
type Answer = Item[] | { status: number; body: unknown };

let server: Server;
let baseUrl = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;

function sse(items: Item[], n: number): string {
	const id = `resp_${n}`;
	const events: object[] = [
		{ type: 'response.created', response: { id, status: 'in_progress', output: [] } }
	];
	items.forEach((item, index) => {
		events.push({ type: 'response.output_item.added', output_index: index, item });
		if (item.type === 'message') {
			const [part] = item.content as { text: string }[];
			events.push({
				type: 'response.output_text.delta',
				output_index: index,
				content_index: 0,
				item_id: item.id,
				delta: part.text
			});
		}
		events.push({ type: 'response.output_item.done', output_index: index, item });
	});
	events.push({
		type: 'response.completed',
		response: {
			id,
			status: 'completed',
			output: items,
			usage: {
				input_tokens: 1000,
				input_tokens_details: { cached_tokens: 800 },
				output_tokens: 40,
				output_tokens_details: { reasoning_tokens: 10 },
				total_tokens: 1040
			}
		}
	});
	return events
		.map((e) => `event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`)
		.join('');
}

beforeAll(async () => {
	server = createServer(async (req, res) => {
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk as Buffer);
		let json: Seen['json'] = null;
		try {
			json = JSON.parse(Buffer.concat(chunks).toString('utf8')) as Seen['json'];
		} catch {
			// no body
		}
		const request: Seen = { path: req.url ?? '', headers: req.headers, json };
		if (req.method !== 'POST' || !request.path.endsWith('/responses')) {
			res.writeHead(404, { 'content-type': 'application/json' });
			res.end('{"error":{"message":"not here"}}');
			return;
		}
		seen.push(request);
		const reply = answer(request);
		if (!Array.isArray(reply)) {
			res.writeHead(reply.status, { 'content-type': 'application/json' });
			res.end(JSON.stringify(reply.body));
			return;
		}
		res.writeHead(200, { 'content-type': 'text/event-stream' });
		res.end(sse(reply, seen.length));
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
});

afterAll(() => {
	server.close();
});

/** Codex's settings for tests: the stand-in as its model provider, with a key of its own. */
function useStandIn(): void {
	mkdirSync(paths.codexHome, { recursive: true });
	writeFileSync(
		join(paths.codexHome, 'config.toml'),
		`model_provider = "standin"

[model_providers.standin]
name = "stand-in"
base_url = "${baseUrl}"
wire_api = "responses"
env_key = "STANDIN_KEY"
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
`
	);
}

beforeEach(() => {
	// nolune keeps Codex in a home of its own, whatever the environment says.
	vi.stubEnv('CODEX_HOME', join(tmpdir(), 'not-nolunes-codex-home'));
	vi.stubEnv('STANDIN_KEY', 'standin-key');
	initConfig();
	updateConfig((c) => {
		c.codexPath = codex ?? undefined;
		// These are about the plan, not auto mode's checks (command-safety.test.ts).
		c.commandMode = 'unrestricted';
	});
	useStandIn();
	seen.length = 0;
	answer = () => ({ status: 500, body: { error: { message: 'unexpected request' } } });
});

afterEach(() => {
	cancelChatGptSignIn();
	vi.unstubAllEnvs();
	vi.resetAllMocks();
});

// --- what the model says ---

let ids = 0;

function said(text: string): Item {
	return {
		id: `msg_${++ids}`,
		type: 'message',
		role: 'assistant',
		status: 'completed',
		content: [{ type: 'output_text', text, annotations: [] }]
	};
}

function thought(text: string): Item {
	return { id: `rs_${++ids}`, type: 'reasoning', summary: [{ type: 'summary_text', text }] };
}

/** A script for Codex's harness that runs nolune's run_command, as models on Codex call tools. */
function runs(command: string): Item {
	return {
		id: `ctc_${++ids}`,
		type: 'custom_tool_call',
		status: 'completed',
		call_id: `call_${ids}`,
		name: 'exec',
		input: `const r = await tools.nolune__run_command(${JSON.stringify({ summary: 'Listing the files', icon: 'folder-open', command })});\ntext(r);`
	};
}

function isTitleRequest(request: Seen): boolean {
	return JSON.stringify(request.json?.input ?? []).includes('You name chats');
}

/** Answers the chat's own requests with `replies` in turn, and title requests with a title. */
function scripted(...replies: Answer[]): void {
	let next = 0;
	answer = (request) => {
		if (isTitleRequest(request)) return [said('Files here')];
		return replies[next++] ?? { status: 500, body: { error: { message: 'no more replies' } } };
	};
}

function chatCalls(): Seen[] {
	return seen.filter((r) => !isTitleRequest(r));
}

/** The text of every message a request sent with `role`, in order. */
function texts(request: Seen, role: string): string[] {
	return (request.json?.input ?? [])
		.filter((i) => i.type === 'message' && i.role === role)
		.flatMap((i) => (i.content as { type: string; text?: string }[]) ?? [])
		.flatMap((c) => (c.text ? [c.text] : []));
}

function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

function planChat() {
	const { user, profile } = makeFamily();
	const preset = makePreset('ChatGPT', 'gpt-6-astra', 'chatgpt-plan');
	const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
	return { user, profile, chat };
}

/** A picture kept in nolune's media store, as a message refers to it. */
function kept(data: Buffer, mime: string) {
	const sha256 = createHash('sha256').update(data).digest('hex');
	return { type: 'media', sha256, mime, bytes: data.length };
}

function rowsOf(conversationId: string) {
	return committedRows(conversationId).map((row) => ({
		kind: row.kind,
		content: JSON.parse(row.content) as unknown
	}));
}

const DOT =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';

describe.skipIf(!codex)('chats on the ChatGPT plan', { timeout: 60_000 }, () => {
	it('runs a turn through Codex, and saves it as nolune does', async () => {
		const { user, chat } = planChat();
		scripted([thought('Listing them.'), runs('ls')], [said('One file: a.txt.')]);
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Files?');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		const rows = rowsOf(chat.id);
		const callId = (rows[1].content as { call_id?: string }[])[1]?.call_id;
		expect(rows).toEqual([
			{ kind: 'human', content: [{ type: 'text', text: 'Anna: Files?' }] },
			{
				kind: 'assistant',
				content: [
					{
						type: 'reasoning',
						id: expect.any(String),
						summary: [{ type: 'summary_text', text: 'Listing them.' }]
					},
					{
						type: 'function_call',
						call_id: callId,
						name: 'run_command',
						arguments: JSON.stringify({
							summary: 'Listing the files',
							icon: 'folder-open',
							command: 'ls'
						}),
						status: 'completed'
					}
				]
			},
			{
				kind: 'tool_results',
				content: [{ type: 'tool_result', callId, content: 'a.txt\n[exit code 0]', isError: false }]
			},
			{
				kind: 'assistant',
				content: [expect.objectContaining({ type: 'message', role: 'assistant' })]
			}
		]);
		expect(committedRows(chat.id).at(-1)?.content).toContain('One file: a.txt.');
		expect(vi.mocked(runCommand).mock.calls[0][0]).toMatchObject({ command: 'ls' });
		// Codex says what a model call used after its commands ran: only the last reply has it.
		const [asked, answered] = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(asked.stopReason).toBe('tool_use');
		expect(asked.usage).toBeNull();
		expect(answered.stopReason).toBe('end_turn');
		expect(JSON.parse(answered.usage ?? '{}')).toEqual({
			input: 200,
			cacheRead: 800,
			cacheWrite: 0,
			output: 40
		});

		const [first, second] = chatCalls();
		// Codex's own model provider and key, from the Codex home nolune keeps it in.
		expect(first.headers.authorization).toBe('Bearer standin-key');
		expect(first.headers.originator).toBe('nolune');
		expect(first.json?.model).toBe('gpt-6-astra');
		expect(first.json?.reasoning).toMatchObject({ effort: 'medium', summary: 'auto' });
		// nolune's prompt, and nolune's tool inside Codex's harness; none of Codex's that act on their own.
		expect(texts(first, 'developer')).toContain(chat.systemPrompt);
		const tools = JSON.stringify(first.json);
		expect(tools).toContain('nolune__run_command');
		for (const tool of ['shell', 'apply_patch', 'web_search', 'view_image', 'spawn_agent']) {
			expect(tools).not.toContain(`"name":"${tool}"`);
			expect(tools).not.toContain(`### \`${tool}\``);
		}
		expect(texts(first, 'user')).toContain('Anna: Files?');
		expect(JSON.stringify(second.json?.input)).toContain('a.txt');

		// Named by the same plan, in a Codex of its own that may still be at it.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Files here'), {
			timeout: 20_000
		});
		const session = getConversation(chat.id)?.providerSession;
		expect(session).toEqual({ id: expect.any(String), sentSeq: 1, provider: 'chatgpt-plan' });
		expect(session?.id).not.toBe(chat.id);
		expect(existsSync(join(tmpdir(), 'not-nolunes-codex-home'))).toBe(false);
	});

	it("resumes the chat's thread with only the new messages", async () => {
		const { user, chat } = planChat();
		scripted([said('Hi Anna.')], [said('Still here.')]);
		let ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hello' });
		kick(chat.id);
		await ended;
		const thread = getConversation(chat.id)?.providerSession?.id;

		ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Again' });
		kick(chat.id);
		await ended;

		const [, second] = chatCalls();
		expect(texts(second, 'user').filter((t) => t.startsWith('Anna:'))).toEqual([
			'Anna: Hello',
			'Anna: Again'
		]);
		expect(texts(second, 'assistant')).toContain('Hi Anna.');
		expect(getConversation(chat.id)?.providerSession).toEqual({
			id: thread,
			sentSeq: 3,
			provider: 'chatgpt-plan'
		});
	});

	it("asks for the nearest effort below the chat's that the model takes", async () => {
		const { user, profile } = makeFamily();
		// In Codex's list, GPT-5.5 goes up to xhigh and GPT-6 Astra to max.
		for (const [model, effort] of [
			['gpt-5.5', 'xhigh'],
			['gpt-6-astra', 'max']
		]) {
			const preset = makePreset(model, model, 'chatgpt-plan');
			const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
			setEffort(chat.id, 'max');
			seen.length = 0;
			scripted([said('Thinking hard.')]);
			const ended = loopEnd(chat.id);
			insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hm' });
			kick(chat.id);
			await ended;
			expect(chatCalls()[0].json?.reasoning?.effort).toBe(effort);
		}
	});

	it("starts over in a new thread, with the chat so far, when Codex lost the chat's", async () => {
		const { user, chat } = planChat();
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'trigger',
			senderName: 'Umbrellas',
			text: 'Opened from a notification.',
			content: JSON.stringify([{ type: 'text', text: '[Notification "Umbrellas"]' }])
		});
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([said('Rain at 4pm: take umbrellas.')])
		});
		const lost = crypto.randomUUID();
		setProviderSession(chat.id, { id: lost, sentSeq: 2, provider: 'chatgpt-plan' });
		scripted([said('Back.')]);
		const ended = loopEnd(chat.id);
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Hello?'
		});
		kick(chat.id);
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		expect(committedRows(chat.id).at(-1)?.content).toContain('Back.');
		const [earlier, asked] = texts(chatCalls()[0], 'user').slice(-2);
		expect(earlier).toBe(
			'[This chat started before you could see it. What was said so far, oldest first:]\n\n[Notification "Umbrellas"]\n\nYou: Rain at 4pm: take umbrellas.'
		);
		expect(asked).toBe('Anna: Hello?');
		const session = getConversation(chat.id)?.providerSession;
		expect(session?.id).not.toBe(lost);
	});

	it('starts a thread of its own when the chat comes from the Claude plan', async () => {
		const { user, chat } = planChat();
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'trigger',
			senderName: 'Umbrellas',
			text: 'Opened from a notification.',
			content: JSON.stringify([{ type: 'text', text: '[Notification "Umbrellas"]' }])
		});
		// Claude Code's session, saved before sessions said whose they are.
		setProviderSession(chat.id, { id: chat.id, sentSeq: 1 });
		scripted([said('Hello from Codex.')]);
		const logged = vi.spyOn(console, 'error');
		const ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		kick(chat.id);
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		// Codex was never asked for Claude Code's session.
		expect(logged).not.toHaveBeenCalledWith(expect.stringContaining('session missing'));
		expect(texts(chatCalls()[0], 'user').at(-2)).toContain('[Notification "Umbrellas"]');
		expect(getConversation(chat.id)?.providerSession).toMatchObject({ provider: 'chatgpt-plan' });
		logged.mockRestore();
	});

	it('sends pictures inline and PDFs as their path, having no Files API', async () => {
		const { user, profile, chat } = planChat();
		const photo = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'dot.png',
			body: Readable.from([Buffer.from(DOT, 'base64')])
		});
		const pdf = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'menu.pdf',
			body: Readable.from([Buffer.from('%PDF-1.4\n% a menu\n')])
		});
		scripted([said('A dot.')]);

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'What are these?', [
			photo.id,
			pdf.id
		]);
		await ended;

		const message = chatCalls()[0].json?.input?.findLast(
			(i) => i.type === 'message' && i.role === 'user'
		);
		expect(message?.content).toEqual([
			{ type: 'input_text', text: expect.stringMatching(/^\[Anna attached dot\.png, saved at /) },
			expect.objectContaining({ type: 'input_image', image_url: `data:image/png;base64,${DOT}` }),
			{
				type: 'input_text',
				text: expect.stringMatching(
					/^\[Anna attached menu\.pdf, saved at .*It isn't shown here: models on the ChatGPT plan don't take PDFs\]$/
				)
			},
			{ type: 'input_text', text: 'Anna: What are these?' }
		]);
		// Named in a Codex of its own, which must end before the next test empties Codex's home.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Files here'), {
			timeout: 20_000
		});
	});

	it('shows the model the pictures a command opened with `nolune view`', async () => {
		const { user, chat } = planChat();
		scripted([runs('nolune view dot.png')], [said('A dot.')]);
		vi.mocked(runCommand).mockImplementationOnce(async (_input, opts) => {
			// What `nolune view dot.png` does inside the command.
			const path = join(mkdtempSync(join(tmpdir(), 'nolune-dot-')), 'dot.png');
			writeFileSync(path, Buffer.from(DOT, 'base64'));
			await viewImage(path, opts.env.NOLUNE_VIEW_DIR ?? '');
			return { content: 'Viewing dot.png\n[exit code 0]', isError: false, exitCode: 0 };
		});
		const ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Look' });
		kick(chat.id);
		await ended;

		const [, second] = chatCalls();
		expect(JSON.stringify(second.json?.input)).toContain(`data:image/png;base64,${DOT}`);
		expect(rowsOf(chat.id)[2].content).toEqual([
			expect.objectContaining({
				content: [
					{ type: 'text', text: 'Viewing dot.png\n[exit code 0]' },
					{ type: 'text', text: expect.stringContaining('Image: ') },
					{ type: 'image', source: kept(Buffer.from(DOT, 'base64'), 'image/png') }
				]
			})
		]);
	});

	it('stops a running command and ends the turn', async () => {
		const { user, chat } = planChat();
		scripted([runs('sleep 60')]);
		vi.mocked(runCommand).mockImplementationOnce(
			(_input, opts) =>
				new Promise((resolve) => {
					opts.signal?.addEventListener('abort', () =>
						resolve({ content: opts.abortReason?.() ?? 'stopped', isError: true, exitCode: null })
					);
				})
		);

		const ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Wait' });
		kick(chat.id);
		await vi.waitFor(() => expect(runCommand).toHaveBeenCalled(), { timeout: 20_000 });
		stop(chat.id, 'Anna');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		const rows = rowsOf(chat.id).slice(1);
		expect(rows.map((row) => row.kind)).toEqual(['assistant', 'tool_results']);
		expect(rows[1].content).toEqual([
			expect.objectContaining({ content: 'Stopped by Anna.', isError: true })
		]);
		// The model wasn't asked to go on.
		expect(chatCalls()).toHaveLength(1);
	});

	it("says what went wrong when the model's service turns Codex down", async () => {
		const { user, chat } = planChat();
		answer = () => ({ status: 400, body: { error: { message: 'This model is not available.' } } });
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
		const ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		kick(chat.id);
		await ended;

		expect(getSnapshot(chat.id).error).toContain('This model is not available.');
		expect(rowsOf(chat.id).map((row) => row.kind)).toEqual(['human']);
		logged.mockRestore();
	});

	it("says so when a preset is added while Codex isn't signed in with ChatGPT", async () => {
		await expect(addPreset({ provider: 'chatgpt-plan', model: 'gpt-6-astra' })).rejects.toThrow(
			/Codex isn't signed in with ChatGPT.*nolune chatgpt-plan setup/
		);
		const status = await chatGptPlanStatus();
		expect(status).toMatchObject({ path: codex, installed: true, account: null, signedIn: null });
		// Codex's own list, without asking the model.
		expect((await listChatGptModels()).length).toBeGreaterThan(0);
		expect(seen).toEqual([]);
	});
});

describe('signing in with ChatGPT through Codex', { timeout: 20_000 }, () => {
	it('signs in with a code entered on another device, which Codex keeps', async () => {
		useFakeCodex();
		expect((await chatGptPlanStatus()).problem).toMatch(/^Codex isn't signed in with ChatGPT/);

		const signIn = await startChatGptSignIn();
		expect(signIn).toMatchObject({
			verificationUrl: 'https://auth.openai.com/codex/device',
			userCode: 'ABCD-1234'
		});
		expect(chatGptSignInState().pending).toMatchObject({ userCode: 'ABCD-1234' });
		writeFileSync(join(paths.codexHome, 'entered'), '');
		await signIn.done;

		expect(chatGptSignInState()).toEqual({ pending: null, signInError: null });
		const status = await chatGptPlanStatus();
		expect(status).toMatchObject({
			installed: true,
			account: { email: 'anna@example.com', plan: 'ChatGPT Plus' },
			signedIn: 'signed in as anna@example.com (ChatGPT Plus)',
			problem: null
		});
		expect(codexRequests()).toContainEqual(
			expect.objectContaining({
				method: 'account/login/start',
				params: { type: 'chatgptDeviceCode' }
			})
		);
		// Codex runs with nolune's settings: its own tools off.
		const [start] = codexRequests() as unknown as { args: string[] }[];
		expect(start.args[0]).toBe('app-server');
		expect(start.args).toContain('features.shell_tool=false');
	});

	it('stops waiting, and Codex stops too, when the sign-in is cancelled', async () => {
		useFakeCodex();
		const signIn = await startChatGptSignIn();
		cancelChatGptSignIn();
		await expect(signIn.done).rejects.toThrow('The sign-in was cancelled.');
		await vi.waitFor(() =>
			expect(codexRequests()).toContainEqual({
				method: 'account/login/cancel',
				params: { loginId: 'login-1' },
				args: expect.any(Array)
			})
		);
		expect(chatGptSignInState()).toEqual({ pending: null, signInError: null });
	});

	it('stops Codex too when the sign-in is cancelled while Codex starts', async () => {
		useFakeCodex();
		const start = vi.spyOn(AppServer, 'start');
		const signingIn = startChatGptSignIn();
		// A second sign-in, a double click or Ctrl-C, before Codex said hello.
		cancelChatGptSignIn();
		await expect(signingIn).rejects.toThrow('The sign-in was cancelled.');
		expect(start).toHaveBeenCalledOnce();
		const server = await start.mock.results[0].value;
		await server.exited;
		// The code Codex asked for goes unused.
		expect(codexRequests().map((r) => r.method)).toEqual([
			'initialize',
			'initialized',
			'account/login/start',
			'account/login/cancel'
		]);
		expect(chatGptSignInState()).toEqual({ pending: null, signInError: null });
		start.mockRestore();
	});

	it('signs Codex out', async () => {
		useFakeCodex({ type: 'chatgpt', email: 'anna@example.com', planType: 'pro' });
		expect((await chatGptPlanStatus()).signedIn).toBe(
			'signed in as anna@example.com (ChatGPT Pro)'
		);
		await signOutChatGpt();
		expect(codexRequests().map((r) => r.method)).toContain('account/logout');
		expect((await chatGptPlanStatus()).signedIn).toBeNull();
	});

	it('says an API key is no ChatGPT plan', async () => {
		useFakeCodex({ type: 'apiKey' });
		const status = await chatGptPlanStatus();
		expect(status.signedIn).toBeNull();
		expect(status.problem).toMatch(/^Codex is signed in with an API key, which bills the OpenAI/);
	});

	it('checks the model Codex offers when a preset is added', async () => {
		useFakeCodex({ type: 'chatgpt', email: null, planType: 'business' });
		const preset = await addPreset({ provider: 'chatgpt-plan', model: 'gpt-6-astra' });
		expect(preset).toMatchObject({
			name: 'gpt-6-astra (chatgpt-plan)',
			provider: 'chatgpt-plan',
			modelContextWindow: null
		});
		// Hidden from Codex's picker, but there.
		await addPreset({ provider: 'chatgpt-plan', model: 'gpt-5.4' });
		await expect(addPreset({ provider: 'chatgpt-plan', model: 'gpt-7' })).rejects.toThrow(
			'Codex has no model "gpt-7" on the ChatGPT plan. It has gpt-6-astra.'
		);
		expect((await chatGptPlanStatus()).signedIn).toBe('signed in to ChatGPT Business');
		// The admin page's picker offers what Codex's own does.
		expect(await listModels('chatgpt-plan')).toEqual([
			{ id: 'gpt-6-astra', name: 'GPT-6 Astra', description: null, contextWindow: null }
		]);
	});
});

describe('the ChatGPT plan without Codex', () => {
	// Where Homebrew and npm put it, which a test can't hide.
	const installed = [
		'/opt/homebrew/bin/codex',
		'/usr/local/bin/codex',
		join(dirname(process.execPath), 'codex')
	].some((p) => existsSync(p));

	it.skipIf(installed)('says how to install it', async () => {
		updateConfig((c) => {
			c.codexPath = undefined;
		});
		const empty = mkdtempSync(join(tmpdir(), 'nolune-no-codex-'));
		vi.stubEnv('PATH', empty);
		vi.stubEnv('HOME', empty);

		const status = await chatGptPlanStatus();
		expect(status).toMatchObject({ path: null, installed: false, account: null });
		expect(status.problem).toMatch(/^Codex isn't installed on this computer/);
		expect(status.problem).toContain('npm install -g @openai/codex');
		expect(status.problem).toContain('nolune chatgpt-plan setup');
		await expect(addPreset({ provider: 'chatgpt-plan', model: 'gpt-6-astra' })).rejects.toThrow(
			/Codex isn't installed/
		);
	});

	it('says where nolune was told it is', async () => {
		const missing = join(tmpdir(), 'no-such-codex');
		updateConfig((c) => {
			c.codexPath = missing;
		});
		const status = await chatGptPlanStatus();
		expect(status).toMatchObject({ path: missing, installed: false, account: null });
		expect(status.problem).toMatch(new RegExp(`^There's no Codex at ${missing}`));
	});
});
