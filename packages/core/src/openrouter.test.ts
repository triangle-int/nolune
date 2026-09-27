import { mkdtempSync, writeFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import { replyBlocks } from './content-blocks.ts';
import {
	appendRow,
	committedRows,
	createConversation,
	getConversation,
	requestMessages,
	toDisplay
} from './conversations.ts';
import { viewImage } from './images.ts';
import { describeApiError } from './models.ts';
import {
	openrouterFiles,
	stopReason,
	streamTurn,
	summarizeUsage,
	toChatMessages
} from './openrouter.ts';
import { addPreset } from './presets.ts';
import { RUN_COMMAND_TOOL, TOOLS, runCommand } from './run-command.ts';
import { onLoopEnd, sendMessage, subscribe, type LiveEvent } from './runner.ts';
import { makeFamily, makePreset, pdfWithPages } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

// --- a stand-in for OpenRouter's API ---

interface Seen {
	method: string;
	path: string;
	key: string | undefined;
	body: string;
	json: Record<string, unknown> | null;
}
type Answer = {
	status?: number;
	headers?: Record<string, string>;
	json?: unknown;
	/** Streamed as server-sent events, then `[DONE]`. */
	chunks?: object[];
};

/** What the stand-in lists at /models: the fields btw reads. */
const MODELS = [
	{
		id: 'anthropic/claude-opus-5.5',
		context_length: 1_000_000,
		top_provider: { context_length: 1_000_000 },
		supported_parameters: ['reasoning', 'tools'],
		architecture: { input_modalities: ['text', 'image', 'file'] }
	},
	{
		id: 'deepseek/deepseek-v4.1-flash',
		context_length: 1_048_576,
		top_provider: { context_length: 1_040_000 },
		supported_parameters: ['reasoning', 'tools'],
		architecture: { input_modalities: ['text', 'image'] }
	},
	{
		id: 'deepseek/deepseek-pro',
		context_length: 1_048_576,
		top_provider: null,
		supported_parameters: ['reasoning', 'tools'],
		architecture: { input_modalities: ['text'] }
	},
	{
		id: 'acme/story-teller',
		context_length: 32_000,
		supported_parameters: ['temperature'],
		architecture: { input_modalities: ['text'] }
	}
];

let server: Server;
let baseUrl = '';
const seen: Seen[] = [];
let answer: (request: Seen) => Answer;

/** The stand-in's Files API: the ids it handed out, and whether uploads fail. */
const stored = new Set<string>();
let uploadsFail = false;

/** As OpenRouter answers OpenAI's SDK, in OpenAI's shape. */
function filesApi(request: Seen): Answer {
	const id = request.path.split('/')[4];
	if (request.method === 'POST') {
		if (uploadsFail)
			return { status: 400, json: { error: { message: 'Invalid file type', code: 400 } } };
		const fileId = `or_file_${stored.size + 1}`;
		stored.add(fileId);
		return { json: { _shape: 'openai', id: fileId, object: 'file', purpose: 'user_data' } };
	}
	if (!stored.has(id)) {
		return { status: 404, json: { error: { message: `File not found: ${id}`, code: 404 } } };
	}
	if (request.method === 'DELETE') {
		stored.delete(id);
		return { json: { _shape: 'openai', id, object: 'file', deleted: true } };
	}
	return { json: { _shape: 'openai', id, object: 'file' } };
}

beforeAll(async () => {
	server = createServer(async (req, res) => {
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk as Buffer);
		const body = Buffer.concat(chunks).toString('utf8');
		let json: Seen['json'] = null;
		try {
			json = JSON.parse(body) as Seen['json'];
		} catch {
			// multipart, or empty
		}
		const request: Seen = {
			method: req.method ?? 'GET',
			path: req.url ?? '',
			key: req.headers.authorization?.replace(/^Bearer /, ''),
			body,
			json
		};
		seen.push(request);
		const reply =
			request.path === '/api/v1/models'
				? { json: { data: MODELS } }
				: request.path.startsWith('/api/v1/files')
					? filesApi(request)
					: answer(request);
		if (reply.chunks) {
			res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream', ...reply.headers });
			// OpenRouter keeps the connection alive with comments while the model starts.
			const text =
				': OPENROUTER PROCESSING\n\n' +
				reply.chunks.map((c) => `data: ${JSON.stringify(c)}\n\n`).join('') +
				'data: [DONE]\n\n';
			// In pieces, so events arrive split across reads.
			for (let i = 0; i < text.length; i += 64) res.write(text.slice(i, i + 64));
			res.end();
		} else {
			res.writeHead(reply.status ?? 200, { 'content-type': 'application/json', ...reply.headers });
			res.end(JSON.stringify(reply.json ?? {}));
		}
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/api/v1`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	vi.stubEnv('OPENROUTER_BASE_URL', baseUrl);
	vi.stubEnv('OPENROUTER_API_KEY', 'sk-or-v1-test-0000000001');
	seen.length = 0;
	stored.clear();
	uploadsFail = false;
	answer = () => ({ status: 404, json: { error: { message: 'Not Found', code: 404 } } });
});

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetAllMocks();
});

function chunk(delta: object, finish: string | null = null) {
	return {
		id: 'gen-1',
		object: 'chat.completion.chunk',
		choices: [{ index: 0, delta, finish_reason: finish }]
	};
}

/** The last chunk, which carries the usage and no choices. */
function usage(prompt: number, cached: number, written = 0, completion = 40) {
	return {
		id: 'gen-1',
		object: 'chat.completion.chunk',
		choices: [],
		usage: {
			prompt_tokens: prompt,
			completion_tokens: completion,
			prompt_tokens_details: { cached_tokens: cached, cache_write_tokens: written },
			cost: 0.0123
		}
	};
}

/** A streamed reply that only says `text`. */
function saying(text: string, used = usage(100, 0)): Answer {
	return {
		chunks: [
			chunk({ role: 'assistant', content: text.slice(0, 3) }),
			chunk({ content: text.slice(3) }),
			chunk({}, 'stop'),
			used
		]
	};
}

/** Claude's thinking, as OpenRouter streams it: text in pieces, then its signature. */
const thinkingChunks = [
	chunk({
		role: 'assistant',
		content: '',
		reasoning: 'Anna wants ',
		reasoning_details: [
			{ type: 'reasoning.text', text: 'Anna wants ', format: 'anthropic-claude-v1', index: 0 }
		]
	}),
	chunk({
		reasoning: 'the files.',
		reasoning_details: [{ type: 'reasoning.text', text: 'the files.', index: 0 }]
	}),
	chunk({ reasoning_details: [{ type: 'reasoning.text', signature: 'sig-1', index: 0 }] })
];
const thought = {
	type: 'reasoning.text',
	text: 'Anna wants the files.',
	format: 'anthropic-claude-v1',
	index: 0,
	signature: 'sig-1'
};
const listArgs = JSON.stringify({
	summary: 'Listing the files',
	icon: 'folder-open',
	command: 'ls'
});
const listCall = {
	id: 'toolu_1',
	type: 'function',
	function: { name: 'run_command', arguments: listArgs }
};
const listing: Answer = {
	chunks: [
		...thinkingChunks,
		chunk({
			tool_calls: [
				{
					index: 0,
					id: 'toolu_1',
					type: 'function',
					function: { name: 'run_command', arguments: '' }
				}
			]
		}),
		chunk({ tool_calls: [{ index: 0, function: { arguments: listArgs.slice(0, 20) } }] }),
		chunk({ tool_calls: [{ index: 0, function: { arguments: listArgs.slice(20) } }] }),
		chunk({}, 'tool_calls'),
		usage(2000, 1500, 400)
	]
};

/** The title request, which isn't streamed. */
const titled = (title: string): Answer => ({
	json: {
		id: 'gen-2',
		object: 'chat.completion',
		choices: [{ index: 0, message: { role: 'assistant', content: title }, finish_reason: 'stop' }],
		usage: { prompt_tokens: 60, completion_tokens: 4 }
	}
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

function openrouterChat(model = 'anthropic/claude-opus-5.5') {
	const { user, profile } = makeFamily();
	const preset = makePreset('Through OpenRouter', model, 'openrouter');
	return {
		user,
		profile,
		chat: createConversation({ profile, presetId: preset.id, userId: user.id })
	};
}

const COMPLETIONS = '/api/v1/chat/completions';
const turns = () => seen.filter((r) => r.path === COMPLETIONS && r.json?.stream);
const DOT =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=';

// --- tests ---

describe('a chat on an OpenRouter model', () => {
	it('runs the agent loop and sends each reply back as it came, reasoning included', async () => {
		const { user, chat } = openrouterChat();
		const replies = [listing, saying('One file: a.txt.', usage(2100, 2000, 0, 12))];
		answer = (req) => {
			if (req.path !== COMPLETIONS) return { status: 404, json: {} };
			return req.json?.stream ? replies.shift()! : titled('Listing files');
		};
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: 'a.txt\n[exit code 0]',
			isError: false,
			exitCode: 0
		});
		const live: LiveEvent[] = [];
		const off = subscribe(chat.id, (event) => live.push(event));

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Files?');
		await ended;
		off();

		expect(turns()).toHaveLength(2);
		const [first, second] = turns().map((r) => r.json!);
		expect(turns()[0].key).toBe('sk-or-v1-test-0000000001');
		const cache = { type: 'ephemeral', ttl: '1h' };
		expect(first).toMatchObject({
			model: 'anthropic/claude-opus-5.5',
			stream: true,
			session_id: chat.id,
			reasoning: { effort: 'medium' },
			// Claude caches only what's marked: the system prompt, and the growing tail.
			cache_control: cache,
			tools: [
				{
					type: 'function',
					function: {
						name: 'run_command',
						description: RUN_COMMAND_TOOL.description,
						parameters: RUN_COMMAND_TOOL.input_schema
					}
				}
			]
		});
		expect(first.messages).toEqual([
			{
				role: 'system',
				content: [{ type: 'text', text: chat.systemPrompt, cache_control: cache }]
			},
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Files?' }] }
		]);
		expect(second.messages).toEqual([
			...(first.messages as unknown[]),
			{ role: 'assistant', content: null, tool_calls: [listCall], reasoning_details: [thought] },
			{ role: 'tool', tool_call_id: 'toolu_1', content: 'a.txt\n[exit code 0]' }
		]);
		// Only appended to: the cached prefix stays byte-identical.
		expect(JSON.stringify(second.messages)).toContain(JSON.stringify(first.messages).slice(1, -1));
		expect(runCommand).toHaveBeenCalledWith(
			expect.objectContaining({ command: 'ls' }),
			expect.anything()
		);

		const saved = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(JSON.parse(saved[0].content)).toEqual([thought, listCall]);
		expect(toDisplay(saved[0])).toMatchObject({
			stopReason: 'tool_use',
			usage: { input: 100, cacheRead: 1500, cacheWrite: 400, output: 40 },
			blocks: [
				{ type: 'thinking', text: 'Anna wants the files.' },
				{
					type: 'tool',
					id: 'toolu_1',
					command: 'ls',
					summary: 'Listing the files',
					icon: 'folder-open'
				}
			]
		});
		expect(JSON.parse(saved[1].content)).toEqual([{ type: 'text', text: 'One file: a.txt.' }]);
		expect(toDisplay(saved[1])).toMatchObject({
			stopReason: 'end_turn',
			blocks: [{ type: 'text', text: 'One file: a.txt.' }]
		});

		// The chat saw the thinking and the call while they streamed.
		const firstTurn = live.slice(
			0,
			live.findIndex((e) => e.type === 'message' && e.replacesLive)
		);
		expect(firstTurn).toContainEqual({
			type: 'live_block',
			index: 1,
			block: { type: 'tool', id: 'toolu_1', text: '' }
		});
		const thinking = firstTurn.flatMap((e) =>
			e.type === 'live_delta' && e.index === 0 ? [e.text] : []
		);
		expect(thinking.join('')).toBe('Anna wants the files.');

		// Named by the same model, with a short request of its own.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Listing files'));
		const naming = seen.find((r) => r.path === COMPLETIONS && !r.json?.stream)!.json;
		expect(naming).toMatchObject({
			model: 'anthropic/claude-opus-5.5',
			messages: [
				{ role: 'system', content: expect.any(String) },
				{ role: 'user', content: '<message>\nFiles?\n</message>' }
			],
			max_tokens: 2048,
			reasoning: { effort: 'low' }
		});
	});

	it("uploads pictures and PDFs to OpenRouter's Files API for a model that takes them", async () => {
		const { user, profile, chat } = openrouterChat();
		const upload = (name: string, data: Buffer) =>
			createUpload({ profileId: profile.id, userId: user.id, name, body: Readable.from([data]) });
		const picture = await upload('dot.png', Buffer.from(DOT, 'base64'));
		const pdf = await upload('menu.pdf', pdfWithPages(2));
		answer = (req) => (req.json?.stream ? saying('A dot and a menu.') : titled('Dot and menu'));

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What are these?', [picture.id, pdf.id]);
		await ended;

		expect(turns()[0].json!.messages).toContainEqual({
			role: 'user',
			content: [
				{
					type: 'text',
					text: expect.stringMatching(/^\[Anna attached dot\.png, saved at [^\]]*\]$/)
				},
				// Chat Completions takes an uploaded picture as a file part too.
				{ type: 'file', file: { file_id: 'or_file_1' } },
				{ type: 'text', text: expect.stringMatching(/^\[Anna attached menu\.pdf, saved at /) },
				{ type: 'file', file: { file_id: 'or_file_2', filename: 'menu.pdf' } },
				{ type: 'text', text: 'Anna: What are these?' }
			]
		});
		const uploads = seen.filter((r) => r.method === 'POST' && r.path === '/api/v1/files');
		expect(uploads.map((r) => /filename="([^"]+)"/.exec(r.body)?.[1])).toEqual([
			'dot.png',
			'menu.pdf'
		]);
		expect(uploads[0].key).toBe('sk-or-v1-test-0000000001');
		const human = committedRows(chat.id).find((row) => row.kind === 'human')!;
		// OpenRouter can't count a PDF's tokens: they're estimated from its pages.
		expect(JSON.parse(human.attachments!)).toMatchObject([
			{ sentAs: 'image' },
			{ sentAs: 'document', tokens: 8000 }
		]);
	});

	it("sends a picture inline when it can't be uploaded, and a PDF as its path", async () => {
		const { user, profile, chat } = openrouterChat();
		const upload = (name: string, data: Buffer) =>
			createUpload({ profileId: profile.id, userId: user.id, name, body: Readable.from([data]) });
		const picture = await upload('dot.png', Buffer.from(DOT, 'base64'));
		const pdf = await upload('menu.pdf', pdfWithPages(2));
		uploadsFail = true;
		answer = (req) => (req.json?.stream ? saying('A dot.') : titled('A dot'));

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What are these?', [picture.id, pdf.id]);
		await ended;

		const parts = (turns()[0].json!.messages as { content: unknown[] }[])[1].content;
		expect(parts[1]).toEqual({
			type: 'image_url',
			image_url: { url: `data:image/png;base64,${DOT}` }
		});
		const human = committedRows(chat.id).find((row) => row.kind === 'human')!;
		expect(JSON.parse(human.attachments!)).toMatchObject([
			{ sentAs: 'image' },
			{ sentAs: 'path', note: "it couldn't be uploaded (Invalid file type)" }
		]);
	});

	it("sends a picture or a PDF as its path to a model that can't take it", async () => {
		const { user, profile, chat } = openrouterChat('deepseek/deepseek-pro');
		const upload = (name: string, data: Buffer) =>
			createUpload({ profileId: profile.id, userId: user.id, name, body: Readable.from([data]) });
		const picture = await upload('dot.png', Buffer.from(DOT, 'base64'));
		const pdf = await upload('menu.pdf', pdfWithPages(2));
		answer = (req) => (req.json?.stream ? saying("I can't see it.") : titled('A dot'));

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What is this?', [picture.id, pdf.id]);
		await ended;

		const parts = (turns()[0].json!.messages as { content: { type: string }[] }[])[1].content;
		expect(parts.map((p) => p.type)).toEqual(['text', 'text', 'text']);
		expect(parts[0]).toEqual({
			type: 'text',
			text: expect.stringContaining(
				"It isn't shown here: deepseek/deepseek-pro can't see pictures]"
			)
		});
		const human = committedRows(chat.id).find((row) => row.kind === 'human')!;
		expect(JSON.parse(human.attachments!)).toMatchObject([
			{ sentAs: 'path', note: "deepseek/deepseek-pro can't see pictures" },
			{ sentAs: 'path', note: "deepseek/deepseek-pro doesn't read PDFs itself" }
		]);
	});

	it('shows the model the pictures a command opened, after its result', async () => {
		const { user, chat } = openrouterChat('deepseek/deepseek-v4.1-flash');
		const replies = [listing, saying('A dot.')];
		answer = (req) => (req.json?.stream ? replies.shift()! : titled('A dot'));
		vi.mocked(runCommand).mockImplementationOnce(async (_input, opts) => {
			// What `btw view dot.png` does inside the command.
			const path = join(mkdtempSync(join(tmpdir(), 'btw-dot-')), 'dot.png');
			writeFileSync(path, Buffer.from(DOT, 'base64'));
			await viewImage(path, opts.env.BTW_VIEW_DIR ?? '');
			return { content: 'Viewing dot.png\n[exit code 0]', isError: false, exitCode: 0 };
		});

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Look');
		await ended;

		// A tool message takes only text: the picture follows in a message of its own.
		const [tool, after] = (turns()[1].json!.messages as unknown[]).slice(-2);
		expect(tool).toEqual({
			role: 'tool',
			tool_call_id: 'toolu_1',
			content: expect.stringMatching(/^Viewing dot\.png\n\[exit code 0\]\nImage: /)
		});
		expect(after).toEqual({
			role: 'user',
			content: [
				{ type: 'text', text: expect.stringMatching(/^Image: /) },
				{ type: 'file', file: { file_id: 'or_file_1' } }
			]
		});
	});
});

describe('the transcript as chat messages', () => {
	it("turns btw's own blocks into messages, and puts OpenRouter's replies back together", () => {
		expect(
			toChatMessages('You are btw.', [
				{
					role: 'user',
					content: [
						{ type: 'text', text: '[Anna attached cat.jpg, saved at /tmp/cat.jpg]' },
						{ type: 'image', source: { type: 'file', file_id: 'or_file_8' } },
						{ type: 'document', source: { type: 'file', file_id: 'or_file_9' }, title: 'menu.pdf' },
						// A picture that couldn't be uploaded.
						{ type: 'image', source: { type: 'base64', media_type: 'image/jpeg', data: '/9j/' } },
						{ type: 'text', text: 'Anna: Look' }
					]
				},
				// A notification someone continued in a chat: btw wrote this reply itself.
				{ role: 'assistant', content: [{ type: 'text', text: 'The parcel arrived.' }] },
				{ role: 'assistant', content: [thought, listCall] as never },
				{
					role: 'user',
					content: [
						{
							type: 'tool_result',
							tool_use_id: 'toolu_1',
							content: [
								{ type: 'text', text: 'Viewing shot.png\n[exit code 0]' },
								{ type: 'text', text: 'Image: shot.png' },
								{
									type: 'image',
									source: { type: 'base64', media_type: 'image/png', data: 'iVBOR' }
								}
							]
						},
						{ type: 'text', text: 'Max: and?' }
					]
				},
				// Only reasoning (cut off): nothing to send.
				{
					role: 'assistant',
					content: [{ type: 'reasoning.encrypted', data: 'enc', index: 0 }] as never
				},
				{ role: 'user', content: 'Anna: Hello?' }
			])
		).toEqual([
			{ role: 'system', content: 'You are btw.' },
			{
				role: 'user',
				content: [
					{ type: 'text', text: '[Anna attached cat.jpg, saved at /tmp/cat.jpg]' },
					{ type: 'file', file: { file_id: 'or_file_8' } },
					{ type: 'file', file: { file_id: 'or_file_9', filename: 'menu.pdf' } },
					{ type: 'image_url', image_url: { url: 'data:image/jpeg;base64,/9j/' } },
					{ type: 'text', text: 'Anna: Look' }
				]
			},
			{ role: 'assistant', content: 'The parcel arrived.' },
			{ role: 'assistant', content: null, tool_calls: [listCall], reasoning_details: [thought] },
			{
				role: 'tool',
				tool_call_id: 'toolu_1',
				content: 'Viewing shot.png\n[exit code 0]\nImage: shot.png'
			},
			{
				role: 'user',
				content: [
					{ type: 'text', text: 'Image: shot.png' },
					{ type: 'image_url', image_url: { url: 'data:image/png;base64,iVBOR' } },
					{ type: 'text', text: 'Max: and?' }
				]
			},
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Hello?' }] }
		]);
	});

	it("reads OpenRouter's replies for the chat and the runner", () => {
		expect(
			replyBlocks([
				thought,
				{ type: 'reasoning.summary', summary: 'Checked the folder.', index: 1 },
				{ type: 'reasoning.encrypted', data: 'enc', index: 2 },
				{ type: 'text', text: 'Here.' },
				{ id: 'call_2', type: 'function', function: { name: 'run_command', arguments: '{"a": ' } }
			])
		).toEqual([
			{ type: 'thinking', text: 'Anna wants the files.' },
			{ type: 'thinking', text: 'Checked the folder.' },
			{ type: 'text', text: 'Here.' },
			// Cut off in the middle: passed on as it is, and run_command refuses it.
			{ type: 'tool_call', id: 'call_2', name: 'run_command', input: '{"a": ' }
		]);
	});

	it('leaves out reasoning from before the system prompt was built again, like thinking', () => {
		const { chat } = openrouterChat();
		const text = (t: string) => JSON.stringify([{ type: 'text', text: t }]);
		appendRow({ conversationId: chat.id, role: 'user', kind: 'trigger', content: text('Hi') });
		const reply = appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([thought, { type: 'text', text: 'Hello.' }])
		});
		expect(requestMessages(committedRows(chat.id), reply.seq)[1]).toEqual({
			role: 'assistant',
			content: [{ type: 'text', text: 'Hello.' }]
		});
		expect(requestMessages(committedRows(chat.id), null)[1].content).toEqual([
			thought,
			{ type: 'text', text: 'Hello.' }
		]);
	});

	it('says why a reply ended, in the words btw stores', () => {
		expect(stopReason('stop', false)).toBe('end_turn');
		expect(stopReason('tool_calls', true)).toBe('tool_use');
		expect(stopReason('length', true)).toBe('max_tokens');
		expect(stopReason('content_filter', false)).toBe('refusal');
		expect(stopReason('stop', false, true)).toBe('refusal');
	});

	it('counts cached tokens apart from the rest of the prompt', () => {
		expect(
			summarizeUsage({
				prompt_tokens: 5000,
				completion_tokens: 70,
				prompt_tokens_details: { cached_tokens: 3000, cache_write_tokens: 1500 }
			})
		).toEqual({ input: 500, cacheRead: 3000, cacheWrite: 1500, output: 70 });
		expect(summarizeUsage(null)).toEqual({ input: 0, cacheRead: 0, cacheWrite: 0, output: 0 });
	});
});

describe('calling OpenRouter', () => {
	const turn = (model = 'anthropic/claude-opus-5.5') =>
		streamTurn({
			model,
			effort: 'high',
			system: 'You are btw.',
			tools: TOOLS,
			cacheTtl: '5m',
			messages: [{ role: 'user', content: [{ type: 'text', text: 'Anna: Hi' }] }],
			cacheKey: 'chat-1',
			signal: new AbortController().signal,
			onEvent: () => {}
		});

	it("marks the cache for Claude only, with the conversation's TTL", async () => {
		answer = () => saying('Hello.');
		await turn();
		await turn('deepseek/deepseek-v4.1-flash');
		const [claude, deepseek] = turns().map((r) => r.json!);
		expect(claude.cache_control).toEqual({ type: 'ephemeral', ttl: '5m' });
		expect((claude.messages as unknown[])[0]).toEqual({
			role: 'system',
			content: [
				{ type: 'text', text: 'You are btw.', cache_control: { type: 'ephemeral', ttl: '5m' } }
			]
		});
		expect(deepseek).not.toHaveProperty('cache_control');
		expect((deepseek.messages as unknown[])[0]).toEqual({
			role: 'system',
			content: 'You are btw.'
		});
		expect(deepseek).toMatchObject({ session_id: 'chat-1', reasoning: { effort: 'high' } });
	});

	it('puts reasoning and calls back together from pieces with and without an index', async () => {
		const call = (id: string | null, args: string) => ({
			...(id ? { id, type: 'function' } : {}),
			function: { ...(id ? { name: 'run_command' } : {}), arguments: args }
		});
		answer = () => ({
			chunks: [
				chunk({ reasoning_details: [{ type: 'reasoning.summary', summary: 'Plan', index: 0 }] }),
				chunk({ reasoning_details: [{ type: 'reasoning.summary', summary: 'ning.' }] }),
				chunk({ reasoning_details: [{ type: 'reasoning.encrypted', data: 'gAAA', id: 'rs_1' }] }),
				chunk({ content: 'Checking.' }),
				chunk({ tool_calls: [call('call_a', '{"command":')] }),
				chunk({ tool_calls: [call(null, '"ls"}')] }),
				chunk({ tool_calls: [call('call_b', '{"command":"pwd"}')] }),
				chunk({}, 'tool_calls'),
				usage(10, 0)
			]
		});
		expect(await turn()).toMatchObject({
			stopReason: 'tool_use',
			content: [
				{ type: 'reasoning.summary', summary: 'Planning.', index: 0 },
				{ type: 'reasoning.encrypted', data: 'gAAA', id: 'rs_1' },
				{ type: 'text', text: 'Checking.' },
				{
					id: 'call_a',
					type: 'function',
					function: { name: 'run_command', arguments: '{"command":"ls"}' }
				},
				{
					id: 'call_b',
					type: 'function',
					function: { name: 'run_command', arguments: '{"command":"pwd"}' }
				}
			]
		});
	});

	it('tries again when the provider behind OpenRouter is down, after the wait it asks for', async () => {
		let calls = 0;
		answer = () =>
			calls++ === 0
				? {
						status: 502,
						headers: { 'retry-after-ms': '1' },
						json: { error: { message: 'Provider returned error', code: 502 } }
					}
				: saying('Hello.');
		expect((await turn()).content).toEqual([{ type: 'text', text: 'Hello.' }]);
		expect(turns()).toHaveLength(2);
	});

	it('explains failures in plain words', async () => {
		const failure = () => turn().catch((err: unknown) => err);

		answer = () => ({ status: 401, json: { error: { message: 'User not found.', code: 401 } } });
		expect(describeApiError(await failure())).toContain("OpenRouter didn't accept the API key.");

		answer = () => ({
			status: 402,
			json: { error: { message: 'Insufficient credits.', code: 402 } }
		});
		expect(describeApiError(await failure())).toBe(
			'OpenRouter needs more credits: Insufficient credits. Add some at https://openrouter.ai/settings/credits.'
		);

		// An error in the middle of the stream, passed on from the provider behind OpenRouter.
		answer = () => ({
			chunks: [
				chunk({ content: 'Hel' }),
				{
					id: 'gen-1',
					object: 'chat.completion.chunk',
					error: {
						code: 502,
						message: 'Provider returned error',
						metadata: {
							provider_name: 'Anthropic',
							raw: '{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}'
						}
					},
					choices: [{ index: 0, delta: { content: '' }, finish_reason: 'error' }]
				}
			]
		});
		expect(describeApiError(await failure())).toBe(
			'OpenRouter: Provider returned error (Anthropic: Overloaded)'
		);

		// One without a status: its code says what it was.
		answer = () => ({
			chunks: [
				{
					id: 'gen-1',
					object: 'chat.completion.chunk',
					error: { code: 402, message: 'Insufficient credits.' },
					choices: [{ index: 0, delta: { content: '' }, finish_reason: 'error' }]
				}
			]
		});
		expect(describeApiError(await failure())).toMatch(/^OpenRouter needs more credits: /);

		answer = () => ({ chunks: [chunk({ content: 'Hel' })] });
		expect(describeApiError(await failure())).toBe(
			'OpenRouter: the reply ended before it was complete.'
		);

		vi.stubEnv('OPENROUTER_API_KEY', '');
		expect(describeApiError(await failure())).toContain('No OpenRouter API key.');
	});
});

describe("OpenRouter's Files API", () => {
	it('uploads with the key, and tells a deleted file from a failure', async () => {
		const id = await openrouterFiles.upload(Buffer.from(DOT, 'base64'), 'a/b:c.png', 'image/png');
		expect(id).toBe('or_file_1');
		const upload = seen.find((r) => r.method === 'POST')!;
		expect(upload.key).toBe('sk-or-v1-test-0000000001');
		expect(upload.body).toContain('filename="a_b_c.png"');
		expect(await openrouterFiles.exists(id)).toBe(true);
		await openrouterFiles.remove(id);
		expect(await openrouterFiles.exists(id)).toBe(false);
		// Already gone: still resolves.
		await expect(openrouterFiles.remove(id)).resolves.toBeUndefined();
		expect(openrouterFiles.account()).toMatch(/^[0-9a-f]{16}$/);

		uploadsFail = true;
		const failed = await openrouterFiles
			.upload(Buffer.from('x'), 'x.txt', 'text/plain')
			.catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('OpenRouter API error 400: Invalid file type');
	});
});

describe("OpenRouter's models", () => {
	it('checks the model when a preset is added, and takes the smaller window it lists', async () => {
		expect(
			await addPreset({ provider: 'openrouter', model: 'anthropic/claude-opus-5.5' })
		).toMatchObject({
			name: 'anthropic/claude-opus-5.5 (openrouter)',
			provider: 'openrouter',
			modelContextWindow: 1_000_000
		});
		expect(seen.find((r) => r.path === '/api/v1/models')?.key).toBe('sk-or-v1-test-0000000001');
		expect(
			await addPreset({ provider: 'openrouter', model: 'deepseek/deepseek-v4.1-flash' })
		).toMatchObject({ modelContextWindow: 1_040_000 });
		// A variant is its model, routed differently.
		expect(
			await addPreset({ provider: 'openrouter', model: 'anthropic/claude-opus-5.5:nitro' })
		).toMatchObject({ modelContextWindow: 1_000_000 });

		await expect(addPreset({ provider: 'openrouter', model: 'acme/story-teller' })).rejects.toThrow(
			"acme/story-teller can't call tools on OpenRouter, and btw needs them to run commands."
		);
		await expect(addPreset({ provider: 'openrouter', model: 'claude-opus-5-5' })).rejects.toThrow(
			'Could not verify model "claude-opus-5-5": Model not found: OpenRouter has no model "claude-opus-5-5".'
		);
	});
});
