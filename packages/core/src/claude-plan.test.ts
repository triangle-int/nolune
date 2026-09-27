import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import { createRequire } from 'node:module';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload, prepareMessage } from './attachments.ts';
import { checkClaudePlan } from './claude-plan.ts';
import { initConfig, updateConfig } from './config.ts';
import {
	appendRow,
	committedRows,
	createConversation,
	getConversation,
	insertQueued,
	setProviderSession
} from './conversations.ts';
import { viewImage } from './images.ts';
import { addPreset } from './presets.ts';
import { RUN_COMMAND_TOOL, runCommand } from './run-command.ts';
import { getSnapshot, kick, onLoopEnd, sendMessage, stop } from './runner.ts';
import { makeFamily, makePreset, pdfWithPages } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

/*
 * These run the real Claude Code, the one that comes with the Agent SDK for this platform,
 * against a stand-in for Anthropic's API. It's signed in with a made-up plan token, so nothing
 * reaches Anthropic and no account is used.
 */

function bundledClaude(): string | null {
	try {
		const require = createRequire(import.meta.url);
		const sdk = createRequire(require.resolve('@anthropic-ai/claude-agent-sdk'));
		const pkg = sdk.resolve(
			`@anthropic-ai/claude-agent-sdk-${process.platform}-${process.arch}/package.json`
		);
		const binary = join(dirname(pkg), 'claude');
		return existsSync(binary) ? binary : null;
	} catch {
		return null;
	}
}

const claude = bundledClaude();
const TOKEN = 'sk-ant-oat01-plan-token-for-tests';

// --- a stand-in for Anthropic's Messages API ---

interface Seen {
	path: string;
	headers: IncomingHttpHeaders;
	json: {
		model?: string;
		system?: { type: string; text: string }[];
		tools?: { name: string; description: string }[];
		messages?: { role: string; content: string | { type: string; text?: string }[] }[];
		stream?: boolean;
	} | null;
}
type Block =
	{ type: 'text'; text: string } | { type: 'tool_use'; id: string; name: string; input: unknown };
type Answer = { stop: string; content: Block[] } | { status: number; message: string };

let server: Server;
let baseUrl = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;
let configDir = '';

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
		if (!request.path.startsWith('/v1/messages')) {
			res.writeHead(404, { 'content-type': 'application/json' });
			res.end('{}');
			return;
		}
		if (request.path.startsWith('/v1/messages/count_tokens')) {
			res.writeHead(200, { 'content-type': 'application/json' });
			res.end(JSON.stringify({ input_tokens: 100 }));
			return;
		}
		seen.push(request);
		const reply = answer(request);
		if ('status' in reply) {
			res.writeHead(reply.status, { 'content-type': 'application/json' });
			res.end(JSON.stringify({ type: 'error', error: { type: 'error', message: reply.message } }));
			return;
		}
		const message = {
			id: `msg_${seen.length}`,
			type: 'message',
			role: 'assistant',
			model: json?.model
		};
		if (!json?.stream) {
			res.writeHead(200, { 'content-type': 'application/json' });
			res.end(
				JSON.stringify({
					...message,
					content: reply.content,
					stop_reason: reply.stop,
					stop_sequence: null,
					usage: { input_tokens: 20, output_tokens: 4 }
				})
			);
			return;
		}
		const events: object[] = [
			{
				type: 'message_start',
				message: {
					...message,
					content: [],
					stop_reason: null,
					stop_sequence: null,
					usage: {
						input_tokens: 120,
						output_tokens: 1,
						cache_read_input_tokens: 800,
						cache_creation_input_tokens: 30
					}
				}
			}
		];
		reply.content.forEach((block, index) => {
			if (block.type === 'text') {
				events.push({
					type: 'content_block_start',
					index,
					content_block: { type: 'text', text: '' }
				});
				events.push({
					type: 'content_block_delta',
					index,
					delta: { type: 'text_delta', text: block.text }
				});
			} else {
				events.push({
					type: 'content_block_start',
					index,
					content_block: { ...block, input: {} }
				});
				events.push({
					type: 'content_block_delta',
					index,
					delta: { type: 'input_json_delta', partial_json: JSON.stringify(block.input) }
				});
			}
			events.push({ type: 'content_block_stop', index });
		});
		events.push({
			type: 'message_delta',
			delta: { stop_reason: reply.stop, stop_sequence: null },
			usage: { output_tokens: 42 }
		});
		events.push({ type: 'message_stop' });
		res.writeHead(200, { 'content-type': 'text/event-stream' });
		res.end(
			events
				.map((e) => `event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`)
				.join('')
		);
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
	configDir = mkdtempSync(join(tmpdir(), 'btw-claude-config-'));
});

afterAll(() => {
	server.close();
	rmSync(configDir, { recursive: true, force: true });
});

beforeEach(() => {
	// Only what the test sets: nothing of a Claude Code this might run inside, or its sign-in.
	for (const name of Object.keys(process.env)) {
		if (/^(CLAUDE|ANTHROPIC_)/.test(name)) vi.stubEnv(name, undefined);
	}
	vi.stubEnv('ANTHROPIC_BASE_URL', baseUrl);
	vi.stubEnv('CLAUDE_CODE_OAUTH_TOKEN', TOKEN);
	vi.stubEnv('CLAUDE_CONFIG_DIR', configDir);
	vi.stubEnv('CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC', '1');
	// An API key in btw's environment must not take the plan's place.
	vi.stubEnv('ANTHROPIC_API_KEY', 'sk-ant-api03-must-not-be-used');
	initConfig();
	updateConfig((c) => {
		c.claudePath = claude ?? undefined;
	});
	seen.length = 0;
	answer = () => ({ status: 500, message: 'unexpected request' });
});

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetAllMocks();
});

const listFiles: Block = {
	type: 'tool_use',
	id: 'toolu_1',
	name: 'mcp__btw__run_command',
	input: { summary: 'Listing the files', icon: 'folder-open', command: 'ls' }
};

/** Answers the chat's own calls with `replies` in turn, and title requests with a title. */
function scripted(...replies: Answer[]): void {
	let next = 0;
	answer = (request) => {
		const system = (request.json?.system ?? []).map((b) => b.text).join('\n');
		if (system.includes('You name chats'))
			return { stop: 'end_turn', content: [{ type: 'text', text: 'Files here' }] };
		return replies[next++] ?? { status: 500, message: 'no more replies' };
	};
}

function chatCalls(): Seen[] {
	return seen.filter((r) => !(r.json?.system ?? []).some((b) => b.text.includes('You name chats')));
}

/** The text of every user message a request sent, in order. */
function userTexts(request: Seen): string[] {
	return (request.json?.messages ?? [])
		.filter((m) => m.role === 'user')
		.flatMap((m) =>
			typeof m.content === 'string'
				? [m.content]
				: m.content.flatMap((b) => (b.type === 'text' && b.text ? [b.text] : []))
		);
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
	const preset = makePreset('Plan', 'claude-opus-5-5', 'claude-plan');
	const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
	return { user, chat };
}

function rowsOf(conversationId: string) {
	return committedRows(conversationId).map((row) => ({
		kind: row.kind,
		content: JSON.parse(row.content) as unknown
	}));
}

describe.skipIf(!claude)('chats on the Claude plan', { timeout: 60_000 }, () => {
	it('runs a turn through Claude Code, signed in with the plan, and saves it as btw does', async () => {
		const { user, chat } = planChat();
		scripted(
			{ stop: 'tool_use', content: [{ type: 'text', text: 'Looking.' }, listFiles] },
			{ stop: 'end_turn', content: [{ type: 'text', text: 'One file: a.txt.' }] }
		);
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'Files?');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		expect(rowsOf(chat.id)).toEqual([
			{ kind: 'human', content: [{ type: 'text', text: 'Anna: Files?' }] },
			{
				kind: 'assistant',
				content: [
					{ type: 'text', text: 'Looking.' },
					{ ...listFiles, name: 'run_command' }
				]
			},
			{
				kind: 'tool_results',
				content: [{ type: 'tool_result', tool_use_id: 'toolu_1', content: 'a.txt\n[exit code 0]' }]
			},
			{ kind: 'assistant', content: [{ type: 'text', text: 'One file: a.txt.' }] }
		]);
		const [reply] = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(reply.stopReason).toBe('tool_use');
		expect(JSON.parse(reply.usage ?? '{}')).toEqual({
			input: 120,
			cacheRead: 800,
			cacheWrite: 30,
			output: 42
		});
		expect(vi.mocked(runCommand).mock.calls[0][0]).toMatchObject({ command: 'ls' });

		const [first, second] = chatCalls();
		// The plan's sign-in, never the API key in btw's environment.
		expect(first.headers.authorization).toBe(`Bearer ${TOKEN}`);
		expect(first.headers['x-api-key']).toBeUndefined();
		expect(first.json?.model).toBe('claude-opus-5-5');
		// btw's own prompt and its one tool, nothing of Claude Code's.
		const system = (first.json?.system ?? []).map((b) => b.text).join('\n');
		expect(system).toContain(chat.systemPrompt);
		expect(first.json?.tools).toEqual([
			expect.objectContaining({
				name: 'mcp__btw__run_command',
				description: RUN_COMMAND_TOOL.description
			})
		]);
		expect(userTexts(first)).toContain('Anna: Files?');
		expect(JSON.stringify(second.json?.messages)).toContain('a.txt');

		// Named by the same plan.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Files here'));
		expect(getConversation(chat.id)?.providerSession).toEqual({ id: chat.id, sentSeq: 1 });
	});

	it("resumes the chat's session with only the new messages", async () => {
		const { user, chat } = planChat();
		scripted(
			{ stop: 'end_turn', content: [{ type: 'text', text: 'Hi Anna.' }] },
			{ stop: 'end_turn', content: [{ type: 'text', text: 'Still here.' }] }
		);
		let ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hello' });
		kick(chat.id);
		await ended;

		ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Again' });
		kick(chat.id);
		await ended;

		const [, second] = chatCalls();
		expect(userTexts(second).filter((t) => t.startsWith('Anna:'))).toEqual([
			'Anna: Hello',
			'Anna: Again'
		]);
		expect(JSON.stringify(second.json?.messages)).toContain('Hi Anna.');
		expect(getConversation(chat.id)?.providerSession).toEqual({ id: chat.id, sentSeq: 3 });
	});

	it("starts over in a new session, with the chat so far, when Claude Code lost the chat's", async () => {
		const { user, chat } = planChat();
		setProviderSession(chat.id, { id: crypto.randomUUID(), sentSeq: 0 });
		scripted({ stop: 'end_turn', content: [{ type: 'text', text: 'Back.' }] });
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
		expect(rowsOf(chat.id).at(-1)).toEqual({
			kind: 'assistant',
			content: [{ type: 'text', text: 'Back.' }]
		});
		expect(userTexts(chatCalls()[0])).toContain('Anna: Hello?');
		const session = getConversation(chat.id)?.providerSession;
		expect(session?.id).not.toBe(chat.id);
	});

	it("gives Claude Code what the chat said before it first saw it, as a notification's chat has", async () => {
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
			content: JSON.stringify([{ type: 'text', text: 'Rain at 4pm: take umbrellas.' }])
		});
		scripted({ stop: 'end_turn', content: [{ type: 'text', text: 'Yes, from 4pm.' }] });
		const ended = loopEnd(chat.id);
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Really?'
		});
		kick(chat.id);
		await ended;

		const [earlier, asked] = userTexts(chatCalls()[0]);
		expect(earlier).toBe(
			'[This chat started before you could see it. What was said so far, oldest first:]\n\n[Notification "Umbrellas"]\n\nYou: Rain at 4pm: take umbrellas.'
		);
		expect(asked).toBe('Anna: Really?');
	});

	it('gives the model an attached PDF as a document', async () => {
		const { user, chat } = planChat();
		scripted({ stop: 'end_turn', content: [{ type: 'text', text: 'Two pages.' }] });
		const form = pdfWithPages(2);
		const pdf = await createUpload({
			profileId: chat.profileId,
			userId: user.id,
			name: 'form.pdf',
			body: Readable.from([form])
		});
		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, { id: user.id, name: 'Anna' }, 'What is this?', [pdf.id]);
		await ended;

		const first = chatCalls()[0].json?.messages?.[0];
		expect(first?.content).toContainEqual(
			expect.objectContaining({
				type: 'document',
				source: { type: 'base64', media_type: 'application/pdf', data: form.toString('base64') }
			})
		);
	});

	it('shows the model the pictures a command opened with `btw view`', async () => {
		const { user, chat } = planChat();
		scripted(
			{ stop: 'tool_use', content: [listFiles] },
			{ stop: 'end_turn', content: [{ type: 'text', text: 'A dot.' }] }
		);
		const dot =
			'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';
		vi.mocked(runCommand).mockImplementationOnce(async (_input, opts) => {
			// What `btw view dot.png` does inside the command.
			const path = join(mkdtempSync(join(tmpdir(), 'btw-dot-')), 'dot.png');
			writeFileSync(path, Buffer.from(dot, 'base64'));
			await viewImage(path, opts.env.BTW_VIEW_DIR ?? '');
			return { content: 'Viewing dot.png\n[exit code 0]', isError: false, exitCode: 0 };
		});
		const ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Look' });
		kick(chat.id);
		await ended;

		const [, second] = chatCalls();
		const result = (second.json?.messages ?? [])
			.flatMap((m) => (typeof m.content === 'string' ? [] : m.content))
			.find((b) => b.type === 'tool_result') as { content?: unknown[] } | undefined;
		expect(result?.content).toContainEqual({
			type: 'image',
			source: { type: 'base64', media_type: 'image/png', data: dot }
		});
		expect(rowsOf(chat.id)[2].content).toEqual([
			expect.objectContaining({
				content: [
					{ type: 'text', text: 'Viewing dot.png\n[exit code 0]' },
					{ type: 'text', text: expect.stringContaining('Image: ') },
					{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: dot } }
				]
			})
		]);
	});

	it('stops a running command and ends the turn', async () => {
		const { user, chat } = planChat();
		scripted({ stop: 'tool_use', content: [listFiles] });
		vi.mocked(runCommand).mockImplementationOnce(
			(_input, opts) =>
				new Promise((resolve) => {
					opts.signal?.addEventListener('abort', () =>
						resolve({ content: opts.abortReason?.() ?? 'stopped', isError: true, exitCode: null })
					);
				})
		);

		const ended = loopEnd(chat.id);
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Files?'
		});
		kick(chat.id);
		await vi.waitFor(() => expect(runCommand).toHaveBeenCalled(), { timeout: 20_000 });
		stop(chat.id, 'Anna');
		await ended;

		expect(getSnapshot(chat.id).error).toBeNull();
		expect(rowsOf(chat.id).slice(1)).toEqual([
			{ kind: 'assistant', content: [{ ...listFiles, name: 'run_command' }] },
			{
				kind: 'tool_results',
				content: [
					{
						type: 'tool_result',
						tool_use_id: 'toolu_1',
						content: 'Stopped by Anna.',
						is_error: true
					}
				]
			}
		]);
		// The model wasn't asked to go on.
		expect(chatCalls()).toHaveLength(1);
	});

	it("says how to sign in when Claude Code's plan sign-in is refused", async () => {
		const { user, chat } = planChat();
		answer = () => ({ status: 401, message: 'OAuth token has expired' });
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
		const ended = loopEnd(chat.id);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		kick(chat.id);
		await ended;

		expect(getSnapshot(chat.id).error).toMatch(/isn't signed in to a Claude plan.*\/login/);
		expect(rowsOf(chat.id).map((row) => row.kind)).toEqual(['human']);
		logged.mockRestore();
	});

	it('checks the sign-in when a preset is added, without calling the model', async () => {
		const status = await checkClaudePlan();
		expect(status.account?.tokenSource).toBe('CLAUDE_CODE_OAUTH_TOKEN');
		expect(status.account?.apiKeySource ?? 'none').toBe('none');

		const preset = await addPreset({ provider: 'claude-plan', model: 'claude-opus-5-5' });
		expect(preset).toMatchObject({
			name: 'claude-opus-5-5 (claude-plan)',
			provider: 'claude-plan',
			modelContextWindow: null
		});
		expect(seen).toEqual([]);
	});
});

describe.skipIf(!claude)('the Claude plan sign-in check', { timeout: 60_000 }, () => {
	it("says so when Claude Code isn't signed in to a plan", async () => {
		vi.stubEnv('CLAUDE_CODE_OAUTH_TOKEN', undefined);
		await expect(addPreset({ provider: 'claude-plan', model: 'claude-opus-5-5' })).rejects.toThrow(
			/isn't signed in to a Claude plan.*\/login/
		);
	});
});

describe('attachments in chats on the Claude plan', () => {
	it('send pictures and PDFs inline, without a Files API', async () => {
		const { user, chat } = planChat();
		const dot =
			'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';
		const upload = (name: string, data: Buffer) =>
			createUpload({
				profileId: chat.profileId,
				userId: user.id,
				name,
				body: Readable.from([data])
			});
		const form = pdfWithPages(2, true);
		const picture = await upload('dot.png', Buffer.from(dot, 'base64'));
		const pdf = await upload('form.pdf', form);
		const unreadable = await upload('locked.pdf', Buffer.from('%PDF-1.4\n%%EOF\n'));
		const tooLong = await upload('manual.pdf', pdfWithPages(20));

		const prepared = await prepareMessage({
			conv: chat,
			profileSlug: 'family',
			senderName: 'Anna',
			text: 'Look',
			uploads: [picture, pdf, unreadable, tooLong],
			earlier: []
		});

		expect(prepared.content).toEqual([
			{ type: 'text', text: expect.stringContaining('Anna attached dot.png') },
			{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: dot } },
			{ type: 'text', text: expect.stringContaining('Anna attached form.pdf') },
			{
				type: 'document',
				source: { type: 'base64', media_type: 'application/pdf', data: form.toString('base64') },
				title: 'form.pdf'
			},
			{
				type: 'text',
				text: expect.stringContaining("couldn't tell how many pages it has")
			},
			{
				type: 'text',
				text: expect.stringContaining("at 20 pages it's about 80,000 tokens, more than")
			},
			{ type: 'text', text: 'Anna: Look' }
		]);
		expect(prepared.attachments.map((a) => [a.sentAs, a.tokens])).toEqual([
			['image', undefined],
			['document', 8000],
			['path', undefined],
			['path', undefined]
		]);
		expect(seen).toEqual([]);
	});
});

describe('chats on the Claude plan without Claude Code', () => {
	it('say that Claude Code is needed', async () => {
		updateConfig((c) => {
			c.claudePath = join(tmpdir(), 'no-such-claude');
		});
		await expect(addPreset({ provider: 'claude-plan', model: 'claude-opus-5-5' })).rejects.toThrow(
			/Claude Code/
		);
	});
});
