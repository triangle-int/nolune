import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { Readable } from 'node:stream';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createUpload } from './attachments.ts';
import { replyBlocks } from './format.ts';
import {
	appendRow,
	committedRows,
	commitQueuedRows,
	createConversation,
	getConversation,
	insertQueued,
	requestMessages,
	setPreset,
	toDisplay
} from './conversations.ts';
import { storeBytes } from './media.ts';
import { describeApiError } from './models.ts';
import {
	countDocumentTokens,
	isListedModel,
	knownContextWindow,
	listModels,
	openaiFiles,
	stopReason,
	streamResponse,
	summarizeUsage,
	toResponsesInput
} from './openai-chat.ts';
import { addPreset } from './presets.ts';
import { RUN_COMMAND_TOOL, TOOLS, runCommand } from './run-command.ts';
import {
	onLoopEnd,
	recoverAfterRestart,
	sendMessage,
	subscribe,
	type LiveEvent
} from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

// --- a stand-in for OpenAI's API ---

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
	events?: object[];
};

let server: Server;
let baseUrl = '';
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
		const reply = answer(request);
		if (reply.events) {
			res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream', ...reply.headers });
			const text = reply.events
				.map((e) => `event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`)
				.join('');
			// In pieces, so events arrive split across reads.
			for (let i = 0; i < text.length; i += 64) res.write(text.slice(i, i + 64));
			res.end();
		} else {
			res.writeHead(reply.status ?? 200, { 'content-type': 'application/json', ...reply.headers });
			res.end(JSON.stringify(reply.json ?? {}));
		}
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/v1`;
});

afterAll(() => {
	server.close();
});

beforeEach(() => {
	vi.stubEnv('OPENAI_BASE_URL', baseUrl);
	vi.stubEnv('OPENAI_API_KEY', 'sk-test-000000000000000001');
	seen.length = 0;
	answer = () => ({ status: 404, json: { error: { message: 'Not found' } } });
});

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetAllMocks();
});

function usage(input: number, cached: number, output = 40) {
	return {
		input_tokens: input,
		input_tokens_details: { cached_tokens: cached },
		output_tokens: output
	};
}

/** A streamed response that sends `output` item by item, the way the Responses API does. */
function streamed(
	output: Record<string, unknown>[],
	opts: { usage?: object; status?: string; reason?: string } = {}
): Answer {
	const events: object[] = [{ type: 'response.created', response: { status: 'in_progress' } }];
	output.forEach((item, index) => {
		events.push({ type: 'response.output_item.added', output_index: index, item });
		if (item.type === 'reasoning') {
			(item.summary as { text: string }[]).forEach((part, j) => {
				events.push({
					type: 'response.reasoning_summary_part.added',
					output_index: index,
					summary_index: j
				});
				events.push({
					type: 'response.reasoning_summary_text.delta',
					output_index: index,
					summary_index: j,
					delta: part.text
				});
			});
		} else if (item.type === 'message') {
			for (const part of item.content as { text: string }[]) {
				events.push({ type: 'response.output_text.delta', output_index: index, delta: part.text });
			}
		} else if (item.type === 'function_call') {
			events.push({
				type: 'response.function_call_arguments.delta',
				output_index: index,
				delta: item.arguments
			});
		}
		events.push({ type: 'response.output_item.done', output_index: index, item });
	});
	const status = opts.status ?? 'completed';
	events.push({
		type: status === 'completed' ? 'response.completed' : 'response.incomplete',
		response: {
			status,
			output,
			usage: opts.usage ?? usage(100, 0),
			incomplete_details: opts.reason ? { reason: opts.reason } : null
		}
	});
	return { events };
}

const thought = {
	id: 'rs_1',
	type: 'reasoning',
	summary: [
		{ type: 'summary_text', text: 'Anna wants the files.' },
		{ type: 'summary_text', text: 'Listing them.' }
	],
	encrypted_content: 'gAAAA-encrypted'
};
const listCall = {
	id: 'fc_1',
	type: 'function_call',
	status: 'completed',
	call_id: 'call_1',
	name: 'run_command',
	arguments: JSON.stringify({ summary: 'Listing the files', icon: 'folder-open', command: 'ls' })
};
function said(text: string) {
	return {
		id: 'msg_1',
		type: 'message',
		role: 'assistant',
		status: 'completed',
		content: [{ type: 'output_text', text, annotations: [] }]
	};
}

/** The title request, which isn't streamed. */
const titled = (title: string): Answer => ({
	json: { status: 'completed', output: [said(title)], usage: usage(60, 0, 4) }
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

function openaiChat() {
	const { user, profile } = makeFamily();
	const preset = makePreset('GPT', 'gpt-6-astra', 'openai');
	return {
		user,
		profile,
		chat: createConversation({ profile, presetId: preset.id, userId: user.id })
	};
}

const turns = () => seen.filter((r) => r.path === '/v1/responses' && r.json?.stream);

// --- tests ---

describe('a chat on an OpenAI model', () => {
	it('runs the agent loop and sends each reply back exactly as it came', async () => {
		const { user, chat } = openaiChat();
		const replies = [
			streamed([thought, listCall], { usage: usage(2000, 1500) }),
			streamed([said('One file: a.txt.')], { usage: usage(2100, 2000, 12) })
		];
		answer = (req) => {
			if (req.path !== '/v1/responses') return { status: 404, json: {} };
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

		const [first, second] = turns().map((r) => r.json!);
		expect(turns()).toHaveLength(2);
		expect(turns()[0].key).toBe('sk-test-000000000000000001');
		expect(first).toMatchObject({
			model: 'gpt-6-astra',
			instructions: chat.systemPrompt,
			store: false,
			stream: true,
			prompt_cache_key: chat.id,
			reasoning: { effort: 'medium', summary: 'auto' },
			include: ['reasoning.encrypted_content'],
			tools: [
				{
					type: 'function',
					name: 'run_command',
					description: RUN_COMMAND_TOOL.description,
					parameters: RUN_COMMAND_TOOL.input_schema,
					strict: false
				}
			]
		});
		expect(first.input).toEqual([
			{ role: 'user', content: [{ type: 'input_text', text: 'Anna: Files?' }] }
		]);
		expect(second.input).toEqual([
			...(first.input as unknown[]),
			thought,
			listCall,
			{ type: 'function_call_output', call_id: 'call_1', output: 'a.txt\n[exit code 0]' }
		]);
		// Only appended to: the cached prefix stays byte-identical.
		expect(JSON.stringify(second.input)).toContain(JSON.stringify(first.input).slice(1, -1));
		expect(runCommand).toHaveBeenCalledWith(
			expect.objectContaining({ command: 'ls' }),
			expect.anything()
		);

		const saved = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(JSON.parse(saved[0].content)).toEqual([thought, listCall]);
		expect(toDisplay(saved[0])).toMatchObject({
			stopReason: 'tool_use',
			usage: { input: 500, cacheRead: 1500, cacheWrite: 0, output: 40 },
			blocks: [
				{ type: 'thinking', text: 'Anna wants the files.\n\nListing them.' },
				{
					type: 'tool',
					id: 'call_1',
					command: 'ls',
					summary: 'Listing the files',
					icon: 'folder-open'
				}
			]
		});
		expect(toDisplay(saved[1])).toMatchObject({
			stopReason: 'end_turn',
			blocks: [{ type: 'text', text: 'One file: a.txt.' }]
		});

		// The chat saw the reasoning and the call while they streamed.
		const firstTurn = live.slice(
			0,
			live.findIndex((e) => e.type === 'message' && e.replacesLive)
		);
		expect(firstTurn).toContainEqual({
			type: 'live_block',
			index: 1,
			block: { type: 'tool', id: 'call_1', text: '' }
		});
		const thinking = firstTurn.flatMap((e) =>
			e.type === 'live_delta' && e.index === 0 ? [e.text] : []
		);
		expect(thinking.join('')).toBe('Anna wants the files.\n\nListing them.');

		// Named by the same model, with a short request of its own.
		await vi.waitFor(() => expect(getConversation(chat.id)?.title).toBe('Listing files'));
		const naming = seen.find((r) => r.path === '/v1/responses' && !r.json?.stream)!.json;
		expect(naming).toMatchObject({
			model: 'gpt-6-astra',
			input: '<message>\nFiles?\n</message>',
			store: false,
			reasoning: { effort: 'low' }
		});
	});

	it("uploads an attached PDF to OpenAI's Files API and sends it as an input file", async () => {
		const { user, profile, chat } = openaiChat();
		const pdf = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'menu.pdf',
			body: Readable.from([Buffer.from('%PDF-1.4\n% a menu\n')])
		});
		answer = (req) => {
			if (req.path === '/v1/files') return { json: { id: 'file-menu', object: 'file' } };
			if (req.path === '/v1/responses/input_tokens') {
				return { json: { object: 'response.input_tokens', input_tokens: 1200 } };
			}
			if (req.path === '/v1/responses')
				return req.json?.stream ? streamed([said('Pizza.')]) : titled('Menu');
			return { status: 404, json: {} };
		};

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What is on it?', [pdf.id]);
		await ended;

		expect(seen.find((r) => r.path === '/v1/files')?.body).toMatch(
			/name="purpose"\r\n\r\nuser_data/
		);
		expect(seen.find((r) => r.path === '/v1/responses/input_tokens')?.json).toEqual({
			model: 'gpt-6-astra',
			input: [{ role: 'user', content: [{ type: 'input_file', file_id: 'file-menu' }] }]
		});
		expect(turns()[0].json!.input).toEqual([
			{
				role: 'user',
				content: [
					{
						type: 'input_text',
						text: expect.stringMatching(/^\[Anna attached menu\.pdf, saved at /)
					},
					{ type: 'input_file', file_id: 'file-menu' },
					{ type: 'input_text', text: 'Anna: What is on it?' }
				]
			}
		]);
		const human = committedRows(chat.id).find((row) => row.kind === 'human')!;
		expect(JSON.parse(human.attachments!)).toMatchObject([{ sentAs: 'document', tokens: 1200 }]);
	});

	it('continues a chat that Claude started, from its text and calls', async () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		const text = (t: string) => ({ type: 'text', text: t });
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'trigger',
			content: JSON.stringify([
				text('[Anna attached a.png, saved at /a.png]'),
				{ type: 'image', source: { type: 'file', file_id: 'file_011photo' } },
				text('Anna: Files?')
			]),
			provider: 'anthropic'
		});
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([
				{ type: 'thinking', thinking: 'Listing them.', signature: 'sig' },
				{ type: 'tool_use', id: 'toolu_1', name: 'run_command', input: { command: 'ls' } }
			]),
			provider: 'anthropic',
			model: 'claude-sonnet-5'
		});
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([{ type: 'tool_result', tool_use_id: 'toolu_1', content: 'a.txt' }]),
			provider: 'anthropic'
		});
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([text('One file.')]),
			provider: 'anthropic',
			model: 'claude-sonnet-5'
		});
		setPreset(chat.id, makePreset('GPT', 'gpt-6-astra', 'openai').id);
		answer = (req) =>
			req.json?.stream ? streamed([said('It is a.txt.')]) : { status: 404, json: {} };

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Which?');
		await ended;

		expect(turns()).toHaveLength(1);
		expect(turns()[0].json).toMatchObject({
			model: 'gpt-6-astra',
			instructions: chat.systemPrompt
		});
		expect(turns()[0].json!.input).toEqual([
			{
				role: 'user',
				content: [
					{ type: 'input_text', text: '[Anna attached a.png, saved at /a.png]' },
					{
						type: 'input_text',
						text: expect.stringMatching(/^\[Picture not shown: it went to the model this chat/)
					},
					{ type: 'input_text', text: 'Anna: Files?' }
				]
			},
			{
				type: 'function_call',
				call_id: 'toolu_1',
				name: 'run_command',
				arguments: '{"command":"ls"}'
			},
			{ type: 'function_call_output', call_id: 'toolu_1', output: 'a.txt' },
			{ role: 'assistant', content: 'One file.' },
			{ role: 'user', content: [{ type: 'input_text', text: 'Anna: Which?' }] }
		]);
		expect(committedRows(chat.id).at(-1)).toMatchObject({
			provider: 'openai',
			model: 'gpt-6-astra'
		});
	});

	it("gives a picture sent while on Claude to OpenAI's Files API after a switch", async () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		// As prepareMessage keeps an attached picture: in the media store, by reference.
		const picture = Buffer.from('a picture of a cat');
		const { sha256, bytes } = storeBytes(picture);
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Look',
			provider: 'anthropic',
			attachments: {
				content: [
					{ type: 'text', text: '[Anna attached cat.jpg, saved at /cat.jpg]' },
					{ type: 'image', source: { type: 'media', sha256, mime: 'image/jpeg', bytes } },
					{ type: 'text', text: 'Anna: Look' }
				],
				files: [],
				media: [],
				uploadIds: []
			}
		});
		commitQueuedRows(chat.id);
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'A cat.' }]),
			provider: 'anthropic',
			model: 'claude-sonnet-5'
		});
		setPreset(chat.id, makePreset('GPT', 'gpt-6-astra', 'openai').id);
		answer = (req) => {
			if (req.path === '/v1/files') return { json: { id: 'file-cat', object: 'file' } };
			if (req.path === '/v1/responses' && req.json?.stream) return streamed([said('Orange.')]);
			return { status: 404, json: {} };
		};

		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'What color?');
		await ended;

		const uploads = seen.filter((r) => r.path === '/v1/files');
		expect(uploads).toHaveLength(1);
		expect(uploads[0].body).toMatch(/name="purpose"\r\n\r\nvision/);
		expect(uploads[0].body).toContain('a picture of a cat');
		expect(turns()[0].json!.input).toEqual([
			{
				role: 'user',
				content: [
					{ type: 'input_text', text: '[Anna attached cat.jpg, saved at /cat.jpg]' },
					{ type: 'input_image', file_id: 'file-cat', detail: 'auto' },
					{ type: 'input_text', text: 'Anna: Look' }
				]
			},
			{ role: 'assistant', content: 'A cat.' },
			{ role: 'user', content: [{ type: 'input_text', text: 'Anna: What color?' }] }
		]);
	});

	it('answers a call left without a result, after a crash or a restart', () => {
		const { chat } = openaiChat();
		const text = (t: string) => JSON.stringify([{ type: 'text', text: t }]);
		appendRow({ conversationId: chat.id, role: 'user', kind: 'trigger', content: text('Files?') });
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([thought, listCall])
		});
		appendRow({ conversationId: chat.id, role: 'user', kind: 'trigger', content: text('And?') });
		expect(toResponsesInput(requestMessages(committedRows(chat.id), null), chat.model)).toEqual([
			{ role: 'user', content: [{ type: 'input_text', text: 'Files?' }] },
			thought,
			listCall,
			{
				type: 'function_call_output',
				call_id: 'call_1',
				output: 'No result came back from this command. It may or may not have run.'
			},
			{ role: 'user', content: [{ type: 'input_text', text: 'And?' }] }
		]);

		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([listCall])
		});
		recoverAfterRestart();
		expect(JSON.parse(committedRows(chat.id).at(-1)!.content)).toEqual([
			{
				type: 'tool_result',
				callId: 'call_1',
				content: 'Not finished: the gateway restarted while this was running.',
				isError: true
			}
		]);
	});
});

describe('the transcript as input items', () => {
	it("turns nolune's own blocks into input items, and keeps OpenAI's own as they came", () => {
		const unreadable = [{ id: 'rs_2', type: 'reasoning', summary: [] }, listCall];
		expect(
			toResponsesInput(
				[
					{
						role: 'user',
						blocks: [
							{ type: 'text', text: '[Anna attached cat.jpg, saved at /tmp/cat.jpg]' },
							{
								type: 'image',
								source: { type: 'uploaded', provider: 'openai', fileId: 'file-cat' }
							},
							{ type: 'text', text: 'Anna: Look' }
						]
					},
					// A notification someone continued in a chat: nolune wrote this reply itself.
					{ role: 'assistant', blocks: [{ type: 'text', text: 'The parcel arrived.' }] },
					// Reasoning without its encrypted content can't be sent back without `store`.
					{
						role: 'assistant',
						blocks: replyBlocks(unreadable),
						native: { provider: 'openai', model: 'gpt-6-astra', content: unreadable }
					},
					{
						role: 'user',
						blocks: [
							{
								type: 'tool_result',
								callId: 'call_1',
								content: [
									{ type: 'text', text: 'Image: shot.png' },
									{ type: 'image', source: { type: 'inline', mime: 'image/png', data: 'iVBOR' } }
								],
								isError: false
							},
							{ type: 'text', text: 'Max: and?' }
						]
					}
				],
				'gpt-6-astra'
			)
		).toEqual([
			{
				role: 'user',
				content: [
					{ type: 'input_text', text: '[Anna attached cat.jpg, saved at /tmp/cat.jpg]' },
					{ type: 'input_image', file_id: 'file-cat', detail: 'auto' },
					{ type: 'input_text', text: 'Anna: Look' }
				]
			},
			{ role: 'assistant', content: 'The parcel arrived.' },
			listCall,
			{
				type: 'function_call_output',
				call_id: 'call_1',
				output: [
					{ type: 'input_text', text: 'Image: shot.png' },
					{ type: 'input_image', image_url: 'data:image/png;base64,iVBOR', detail: 'auto' }
				]
			},
			{ role: 'user', content: [{ type: 'input_text', text: 'Max: and?' }] }
		]);
	});

	it("reads replies in either provider's format", () => {
		expect(
			replyBlocks([
				{ type: 'thinking', thinking: 'Hmm.', signature: 'sig' },
				{ type: 'text', text: 'Checking.' },
				{ type: 'tool_use', id: 'toolu_1', name: 'run_command', input: { command: 'ls' } }
			])
		).toEqual([
			{ type: 'reasoning', text: 'Hmm.' },
			{ type: 'text', text: 'Checking.' },
			{ type: 'tool_call', id: 'toolu_1', name: 'run_command', input: { command: 'ls' } }
		]);
		expect(
			replyBlocks([
				{ type: 'message', content: [{ type: 'refusal', refusal: "I can't help with that." }] },
				{ type: 'function_call', call_id: 'call_2', name: 'run_command', arguments: '{"command": ' }
			])
		).toEqual([
			{ type: 'text', text: "I can't help with that." },
			// Cut off in the middle: passed on as it is, and run_command refuses it.
			{ type: 'tool_call', id: 'call_2', name: 'run_command', input: '{"command": ' }
		]);
	});

	it('says why a reply ended, in the words nolune stores', () => {
		expect(stopReason({ status: 'completed', output: [said('Hi.')] })).toBe('end_turn');
		expect(stopReason({ status: 'completed', output: [thought, listCall] })).toBe('tool_use');
		expect(
			stopReason({ status: 'incomplete', incomplete_details: { reason: 'max_output_tokens' } })
		).toBe('max_tokens');
		expect(
			stopReason({ status: 'incomplete', incomplete_details: { reason: 'content_filter' } })
		).toBe('refusal');
		expect(
			stopReason({
				status: 'completed',
				output: [{ type: 'message', content: [{ type: 'refusal', refusal: 'No.' }] }]
			})
		).toBe('refusal');
	});

	it('counts cached tokens apart from the rest of the prompt', () => {
		expect(
			summarizeUsage({
				input_tokens: 5000,
				input_tokens_details: { cached_tokens: 3000, cache_write_tokens: 1500 },
				output_tokens: 70
			})
		).toEqual({ input: 500, cacheRead: 3000, cacheWrite: 1500, output: 70 });
		expect(summarizeUsage(null)).toEqual({ input: 0, cacheRead: 0, cacheWrite: 0, output: 0 });
	});
});

describe('calling OpenAI', () => {
	const turn = (model = 'gpt-6-astra') =>
		streamResponse({
			model,
			effort: 'high',
			system: 'You are nolune.',
			tools: TOOLS,
			messages: [{ role: 'user', blocks: [{ type: 'text', text: 'Anna: Hi' }] }],
			cacheKey: 'chat-1',
			signal: new AbortController().signal,
			onEvent: () => {}
		});

	it('goes on without reasoning summaries for an organization that must be verified first', async () => {
		vi.stubEnv('OPENAI_API_KEY', 'sk-test-unverified-00000001');
		answer = (req) =>
			(req.json?.reasoning as { summary?: string }).summary
				? {
						status: 400,
						json: {
							error: {
								message: 'Your organization must be verified to generate reasoning summaries.',
								code: 'unsupported_value'
							}
						}
					}
				: streamed([said('Hello.')]);

		expect((await turn()).output).toEqual([said('Hello.')]);
		expect((await turn()).output).toEqual([said('Hello.')]);
		expect(turns().map((r) => r.json!.reasoning)).toEqual([
			{ effort: 'high', summary: 'auto' },
			{ effort: 'high' },
			{ effort: 'high' }
		]);
	});

	it('tries again when OpenAI is overloaded, after the wait it asks for', async () => {
		let calls = 0;
		answer = () =>
			calls++ === 0
				? {
						status: 503,
						headers: { 'retry-after-ms': '1' },
						json: { error: { message: 'Overloaded' } }
					}
				: streamed([said('Hello.')]);
		expect((await turn()).output).toEqual([said('Hello.')]);
		expect(turns()).toHaveLength(2);
	});

	it("doesn't send reasoning settings to models that take none", async () => {
		answer = () => streamed([said('Hello.')]);
		await turn('gpt-4.1');
		expect(turns()[0].json).not.toHaveProperty('reasoning');
		expect(turns()[0].json).not.toHaveProperty('include');
	});

	it('explains failures in plain words', async () => {
		answer = () => ({ status: 401, json: { error: { message: 'Incorrect API key provided' } } });
		const rejected = await turn().catch((err: unknown) => err);
		expect(describeApiError(rejected)).toContain("OpenAI didn't accept the API key.");

		answer = () => ({
			events: [
				{ type: 'response.failed', response: { error: { message: 'The server had an error' } } }
			]
		});
		const failed = await turn().catch((err: unknown) => err);
		expect(describeApiError(failed)).toBe('OpenAI: The server had an error');

		vi.stubEnv('OPENAI_API_KEY', '');
		const missing = await turn().catch((err: unknown) => err);
		expect(describeApiError(missing)).toContain('No OpenAI API key.');
	});
});

describe("OpenAI's Files API and models", () => {
	it('uploads pictures for vision, and tells a deleted file from a failure', async () => {
		answer = (req) => {
			if (req.method === 'POST' && req.path === '/v1/files') return { json: { id: 'file-1' } };
			if (req.path === '/v1/files/file-1') return { json: { id: 'file-1' } };
			if (req.path === '/v1/files/file-down')
				return { status: 400, json: { error: { message: 'Bad' } } };
			return { status: 404, json: { error: { message: 'No such File object' } } };
		};
		expect(await openaiFiles.upload(Buffer.from('png'), 'cat.png', 'image/png')).toBe('file-1');
		expect(seen[0].body).toMatch(/name="purpose"\r\n\r\nvision/);
		expect(seen[0].body).toContain('filename="cat.png"');
		expect(await openaiFiles.exists('file-1')).toBe(true);
		expect(await openaiFiles.exists('file-gone')).toBe(false);
		await expect(openaiFiles.exists('file-down')).rejects.toThrow('Bad');
		await expect(openaiFiles.remove('file-gone')).resolves.toBeUndefined();
		expect(openaiFiles.account()).toMatch(/^[0-9a-f]{16}$/);
	});

	it.each([
		['gpt-5.4', 1_050_000],
		['gpt-5.5', 1_050_000],
		['gpt-5.5-pro', 1_050_000],
		['gpt-5.5-2026-04-23', 1_050_000],
		['gpt-5.6-terra', 1_050_000],
		['gpt-6-astra', 1_050_000],
		['gpt-6-luna-pro', 1_050_000],
		['gpt-10-sol', 1_050_000],
		// Smaller, older or unknown: a guess could be too large, so none.
		['gpt-5.4-mini', null],
		['gpt-5.5-nano', null],
		['gpt-5.5-codex', null],
		['gpt-6-astra-mini', null],
		['gpt-5.2', null],
		['gpt-5', null],
		['gpt-4.1', null],
		['o3', null],
		['my-proxy-model', null]
	])('knows %s has a window of %s', (model, window) => {
		expect(knownContextWindow(model)).toBe(window);
	});

	it.each([
		['gpt-6-astra', true],
		['gpt-6-luna-pro', true],
		['gpt-5.6-terra', true],
		['gpt-5.6-sol-2026-02-16', true],
		['gpt-7', true],
		// Older generations: typed, not listed.
		['gpt-5.5', false],
		['gpt-5.4-mini', false],
		['gpt-5.3-codex', false],
		['gpt-4o', false],
		['o3', false],
		['chatgpt-4o-latest', false],
		// Not for chats.
		['gpt-6-astra-audio', false],
		['gpt-6-realtime', false],
		['gpt-6-chat-latest', false],
		['gpt-realtime-2.1', false],
		['gpt-image-2.5-flare', false],
		['whisper-1', false]
	])('lists %s: %s', (model, listed) => {
		expect(isListedModel(model)).toBe(listed);
	});

	it('lists GPT-5.6 and GPT-6, the newest first, without dated snapshots of listed ones', async () => {
		const models = (...ids: string[]) =>
			ids.map((id, i) => ({ id, object: 'model', created: 1_700_000_000 + i, owned_by: 'openai' }));
		answer = (req) =>
			req.path === '/v1/models'
				? {
						json: {
							object: 'list',
							data: models(
								'gpt-5.6-luna',
								'gpt-5.5',
								'whisper-1',
								'gpt-6-astra-2026-09-03',
								'gpt-6-astra',
								'gpt-5.6-terra',
								'gpt-5.4-mini',
								'gpt-image-2.5-flare',
								'gpt-5.6-sol-2026-02-16',
								'o3'
							)
						}
					}
				: { status: 404, json: { error: { message: 'Not found' } } };
		const flagship = { name: null, description: null, contextWindow: 1_050_000 };
		expect(await listModels()).toEqual([
			// Only a dated one of Sol here, so that's the one.
			{ id: 'gpt-5.6-sol-2026-02-16', ...flagship },
			{ id: 'gpt-5.6-terra', ...flagship },
			{ id: 'gpt-6-astra', ...flagship },
			{ id: 'gpt-5.6-luna', ...flagship }
		]);

		// A compatible server's own names aren't OpenAI's: all of them.
		answer = () => ({ json: { object: 'list', data: models('llama-4', 'qwen-3') } });
		expect((await listModels()).map((m) => m.id)).toEqual(['qwen-3', 'llama-4']);
	});

	it('counts what a PDF costs with the input token endpoint', async () => {
		answer = () => ({ json: { object: 'response.input_tokens', input_tokens: 4321 } });
		expect(await countDocumentTokens('gpt-6-astra', 'file-9')).toBe(4321);
	});

	it("checks the model when a preset is added, and knows a flagship's window", async () => {
		answer = (req) =>
			req.path === '/v1/models/gpt-6-astra' || req.path === '/v1/models/gpt-5.4-mini'
				? { json: { id: req.path.split('/').at(-1), object: 'model' } }
				: { status: 404, json: { error: { message: 'The model `gpt-9` does not exist' } } };

		expect(await addPreset({ provider: 'openai', model: 'gpt-6-astra' })).toMatchObject({
			name: 'gpt-6-astra (openai)',
			provider: 'openai',
			modelContextWindow: 1_050_000
		});
		expect(await addPreset({ provider: 'openai', model: 'gpt-5.4-mini' })).toMatchObject({
			modelContextWindow: null
		});
		await expect(addPreset({ provider: 'openai', model: 'gpt-9' })).rejects.toThrow(
			'Could not verify model "gpt-9": Model not found: The model `gpt-9` does not exist'
		);
		await expect(addPreset({ provider: 'mistral', model: 'large' })).rejects.toThrow(
			'Unsupported provider "mistral"'
		);
	});
});
