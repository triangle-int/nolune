import { randomUUID } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	deleteConversation,
	deleteHiddenConversations,
	foundText,
	getConversation,
	getConversationForUser,
	heldFileProviders,
	insertQueued,
	lastCommittedRow,
	listConversations,
	queuedRows,
	replaceTitle,
	replyText,
	requestMessages,
	setHidden,
	setPreset,
	toDisplay,
	touchConversation,
	type Conversation,
	type MessageRow
} from './conversations.ts';
import { eq } from 'drizzle-orm';
import { toAnthropicMessages } from './anthropic.ts';
import { getDb } from './db/index.ts';
import { modelPreset, upload } from './db/schema.ts';
import { toResponsesInput } from './openai-chat.ts';
import { removePreset } from './presets.ts';
import { makeFamily, makePreset, makeUser } from './test/fixtures.ts';
import { getRun, queueWake, updateRun } from './triggers.ts';

afterEach(() => {
	vi.useRealTimers();
});

/** The transcript as Claude gets it. */
function forClaude(rows: MessageRow[], promptChangedAtSeq: number | null = null) {
	return toAnthropicMessages(requestMessages(rows, promptChangedAtSeq));
}

function newChat(options: { hidden?: boolean; title?: string } = {}) {
	const { user, profile } = makeFamily();
	const preset = makePreset();
	const chat = createConversation({ profile, presetId: preset.id, userId: user.id, ...options });
	return { user, profile, preset, chat };
}

describe('createConversation', () => {
	it('keeps a copy of the model and prompt, so later preset changes leave it alone', () => {
		const { profile, preset, chat } = newChat();
		expect(chat).toMatchObject({
			profileId: profile.id,
			presetName: preset.name,
			model: preset.model,
			contextWindow: 200_000,
			effort: 'medium',
			hidden: false
		});
		expect(chat.systemPrompt).toContain(`profiles/${profile.slug}`);

		removePreset(preset.id);
		expect(getConversation(chat.id)).toEqual({ ...chat, presetId: null });
	});

	it('needs a preset that exists', () => {
		const { user, profile } = makeFamily();
		expect(() => createConversation({ profile, presetId: 'nope', userId: user.id })).toThrow(
			'Unknown model preset'
		);
	});
});

describe('listing conversations', () => {
	it('lists visible conversations, most recently active first', () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const { user, profile, preset, chat: first } = newChat({ title: 'First' });
		const make = (title: string, hidden = false) => {
			vi.advanceTimersByTime(60_000);
			return createConversation({ profile, presetId: preset.id, userId: user.id, title, hidden });
		};
		make('Second');
		const background = make('Background run', true);
		expect(listConversations(profile.id).map((c) => c.title)).toEqual(['Second', 'First']);

		vi.advanceTimersByTime(60_000);
		touchConversation(first.id, 'First, renamed');
		setHidden(background.id, false);
		expect(listConversations(profile.id).map((c) => c.title)).toEqual([
			'First, renamed',
			'Background run',
			'Second'
		]);
	});

	it('shows a conversation only to members of its profile', () => {
		const { user, chat } = newChat();
		const max = makeUser('Max');
		expect(getConversationForUser(chat.id, user.id)?.conversation.id).toBe(chat.id);
		expect(getConversationForUser(chat.id, max.id)).toBeUndefined();
	});
});

describe('messages', () => {
	it('keeps queued messages out of the transcript until they are committed, in order', () => {
		const { user, chat } = newChat();
		const trigger = appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'trigger',
			content: '[]'
		});
		const first = insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'hi'
		});
		const second = insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'and?'
		});

		expect(trigger.seq).toBe(1);
		expect(JSON.parse(first.content)).toEqual([{ type: 'text', text: 'Anna: hi' }]);
		expect(queuedRows(chat.id).map((r) => r.id)).toEqual([first.id, second.id]);
		expect(committedRows(chat.id).map((r) => r.id)).toEqual([trigger.id]);

		expect(commitQueuedRows(chat.id).map((r) => [r.id, r.seq])).toEqual([
			[first.id, 2],
			[second.id, 3]
		]);
		expect(commitQueuedRows(chat.id)).toEqual([]);
		const reply = appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: '[]'
		});
		expect(reply.seq).toBe(4);
		expect(lastCommittedRow(chat.id)?.id).toBe(reply.id);
		expect(queuedRows(chat.id)).toEqual([]);
	});

	it('sends each uploaded file once', () => {
		const { user, profile, chat } = newChat();
		const uploadId = randomUUID();
		getDb()
			.insert(upload)
			.values({
				id: uploadId,
				profileId: profile.id,
				userId: user.id,
				name: 'a.txt',
				sha256: 'abc',
				mime: 'text/plain',
				bytes: 1
			})
			.run();
		const send = () =>
			insertQueued({
				conversationId: chat.id,
				senderId: user.id,
				senderName: 'Anna',
				text: 'see attached',
				attachments: { content: [], files: [], media: [], uploadIds: [uploadId] }
			});

		expect(send().attachments).toBe('[]');
		expect(() => send()).toThrow('An attached file was already sent.');
		expect(queuedRows(chat.id)).toHaveLength(1);
	});

	it('shows rows the way the chat draws them', () => {
		const { user, chat } = newChat();
		const human = insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'weather?'
		});
		const assistant = appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([
				{ type: 'thinking', thinking: 'Check the forecast.', signature: '' },
				{ type: 'text', text: '  ' },
				{
					type: 'tool_use',
					id: 't1',
					name: 'run_command',
					input: { command: 'curl wttr.in', summary: ' Checking ' }
				},
				{ type: 'text', text: 'Sunny.' }
			]),
			stopReason: 'end_turn',
			usage: { input: 10, cacheRead: 0, cacheWrite: 0, output: 5 }
		});
		const results = appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([
				{ type: 'tool_result', tool_use_id: 't1', content: 'Sunny +21°C', is_error: false }
			])
		});

		expect(toDisplay(human)).toMatchObject({
			kind: 'human',
			senderName: 'Anna',
			text: 'weather?',
			queued: true
		});
		expect(toDisplay(assistant)).toMatchObject({
			kind: 'assistant',
			blocks: [
				{ type: 'thinking', text: 'Check the forecast.' },
				{ type: 'tool', id: 't1', command: 'curl wttr.in', summary: 'Checking' },
				{ type: 'text', text: 'Sunny.' }
			],
			stopReason: 'end_turn',
			usage: { input: 10, output: 5 }
		});
		expect(toDisplay(results)).toMatchObject({
			kind: 'tool_results',
			results: [{ id: 't1', output: 'Sunny +21°C', isError: false }]
		});
		expect(replyText(assistant)).toBe('Sunny.');
		expect(foundText([human, assistant, results])).toBe('Anna: weather?\nSunny +21°C');
	});
});

describe('requestMessages', () => {
	const calls = (...ids: string[]) =>
		ids.map((id) => ({ type: 'tool_use', id, name: 'run_command', input: { command: 'ls' } }));
	const result = (id: string, content: string, isError = false) => ({
		type: 'tool_result',
		tool_use_id: id,
		content,
		...(isError ? { is_error: true } : {})
	});
	const noResult = (id: string) =>
		result(id, 'No result came back from this command. It may or may not have run.', true);

	function chatWith(...rows: { role: 'user' | 'assistant'; content: unknown[] }[]) {
		const { chat, user } = newChat();
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		commitQueuedRows(chat.id);
		for (const row of rows) {
			appendRow({
				conversationId: chat.id,
				role: row.role,
				kind: row.role === 'assistant' ? 'assistant' : 'tool_results',
				content: JSON.stringify(row.content)
			});
		}
		return { chat, user };
	}

	function say(chat: { id: string }, user: { id: string }, text: string) {
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text });
		commitQueuedRows(chat.id);
	}

	it('sends a healthy transcript exactly as stored', () => {
		const { chat, user } = chatWith(
			{ role: 'assistant', content: calls('t1', 't2') },
			{ role: 'user', content: [result('t1', 'a'), result('t2', 'b')] },
			{ role: 'assistant', content: [{ type: 'text', text: 'Done.' }] }
		);
		say(chat, user, 'Thanks');
		const rows = committedRows(chat.id);
		// Byte for byte: key order too.
		expect(JSON.stringify(forClaude(rows))).toBe(
			JSON.stringify(rows.map((row) => ({ role: row.role, content: JSON.parse(row.content) })))
		);
	});

	it('leaves out a second result for the same call', () => {
		// The results of a command that was still running when the dev server reloaded, after
		// the ones the reload's recovery wrote.
		const { chat, user } = chatWith(
			{ role: 'assistant', content: calls('t1', 't2') },
			{ role: 'user', content: [result('t1', 'Not finished.', true), result('t2', 'x', true)] },
			{ role: 'user', content: [result('t1', 'a'), result('t2', 'b')] }
		);
		say(chat, user, 'Hello?');
		expect(forClaude(committedRows(chat.id)).slice(1)).toEqual([
			{ role: 'assistant', content: calls('t1', 't2') },
			{ role: 'user', content: [result('t1', 'Not finished.', true), result('t2', 'x', true)] },
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Hello?' }] }
		]);
	});

	it('answers calls that never got a result before what comes next', () => {
		const { chat, user } = chatWith(
			{ role: 'assistant', content: calls('t1', 't2') },
			{ role: 'user', content: [result('t1', 'a')] }
		);
		expect(forClaude(committedRows(chat.id)).slice(1)).toEqual([
			{ role: 'assistant', content: calls('t1', 't2') },
			{ role: 'user', content: [result('t1', 'a')] },
			{ role: 'user', content: [noResult('t2')] }
		]);

		say(chat, user, 'Hello?');
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([result('t2', 'late'), result('t9', 'from nowhere')])
		});
		expect(forClaude(committedRows(chat.id)).slice(1)).toEqual([
			{ role: 'assistant', content: calls('t1', 't2') },
			{ role: 'user', content: [result('t1', 'a')] },
			{ role: 'user', content: [noResult('t2'), { type: 'text', text: 'Anna: Hello?' }] }
		]);
	});

	it('answers calls of a reply followed straight by another', () => {
		const { chat } = chatWith(
			{ role: 'assistant', content: calls('t1') },
			{ role: 'assistant', content: [{ type: 'text', text: 'Hm.' }] }
		);
		expect(forClaude(committedRows(chat.id)).slice(1)).toEqual([
			{ role: 'assistant', content: calls('t1') },
			{ role: 'user', content: [noResult('t1')] },
			{ role: 'assistant', content: [{ type: 'text', text: 'Hm.' }] }
		]);
	});
});

describe('switching models', () => {
	const thinking = { type: 'thinking', thinking: 'Anna wants the files.', signature: 'sig' };
	const listCall = { type: 'tool_use', id: 't1', name: 'run_command', input: { command: 'ls' } };
	const listed = { type: 'tool_result', tool_use_id: 't1', content: 'a.txt' };
	const photo = { type: 'image', source: { type: 'file', file_id: 'file_photo' } };
	const inline = {
		type: 'image',
		source: { type: 'base64', media_type: 'image/png', data: 'iVBORw0KGgo=' }
	};
	const note =
		"[Picture not shown: it went to the model this chat used before, and this model can't open that copy. The line before this says where its file is; `btw view` shows it again.]";

	/** A reply the chat's current model wrote. */
	function answer(chat: Conversation, content: unknown[]) {
		return appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify(content),
			provider: chat.provider,
			model: chat.model
		});
	}

	/** Anna's photo and a command that listed files, on Claude, with its reply's thinking. */
	function claudeChat() {
		const { user, chat } = newChat();
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Files?',
			provider: 'anthropic',
			attachments: {
				content: [
					{ type: 'text', text: '[Anna attached a.png, saved at /a.png]' },
					{
						type: 'image',
						source: { type: 'uploaded', provider: 'anthropic', fileId: 'file_photo' }
					}
				],
				files: [],
				media: [],
				uploadIds: []
			}
		});
		commitQueuedRows(chat.id);
		answer(chat, [thinking, { type: 'text', text: 'Looking.' }, listCall]);
		// As results were stored before btw's own format.
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([
				{ ...listed, content: [{ type: 'text', text: 'Image: /b.png' }, photo, inline] }
			]),
			provider: 'anthropic'
		});
		answer(chat, [{ type: 'text', text: 'Two files.' }]);
		return { user, chat };
	}

	it('takes the new model from the next call on, and keeps who wrote each reply', () => {
		const { chat } = claudeChat();
		const gpt = makePreset('GPT', 'gpt-6-astra', 'openai');
		const switched = setPreset(chat.id, gpt.id);
		expect(switched).toMatchObject({
			presetId: gpt.id,
			presetName: 'GPT',
			provider: 'openai',
			model: 'gpt-6-astra',
			contextWindow: 200_000
		});
		expect(getConversation(chat.id)).toEqual(switched);
		const replies = committedRows(chat.id).filter((row) => row.kind === 'assistant');
		expect(replies.map((row) => toDisplay(row))).toMatchObject([
			{ model: 'claude-sonnet-5' },
			{ model: 'claude-sonnet-5' }
		]);
		expect(() => setPreset(chat.id, 'nope')).toThrow('That model was removed');
	});

	it('refuses a model whose window the chat has already outgrown', () => {
		const { chat } = claudeChat();
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'Long.' }]),
			usage: { input: 1_000, cacheRead: 150_000, cacheWrite: 2_000, output: 500 }
		});
		const small = makePreset('Small', 'claude-haiku-4-5');
		getDb()
			.update(modelPreset)
			.set({ modelContextWindow: 100_000 })
			.where(eq(modelPreset.id, small.id))
			.run();
		expect(() => setPreset(chat.id, small.id)).toThrow(
			'This chat is already about 153,500 tokens, more than Small can read (100,000).'
		);
		expect(getConversation(chat.id)?.presetName).toBe('Sonnet');
	});

	it("sends another Claude model the replies as they are: the API leaves out thinking it can't read", () => {
		const { chat } = claudeChat();
		setPreset(chat.id, makePreset('Opus', 'claude-opus-5-5').id);
		expect(forClaude(committedRows(chat.id))).toEqual([
			{
				role: 'user',
				content: [{ type: 'text', text: '[Anna attached a.png, saved at /a.png]' }, photo]
			},
			{ role: 'assistant', content: [thinking, { type: 'text', text: 'Looking.' }, listCall] },
			{
				role: 'user',
				content: [{ ...listed, content: [{ type: 'text', text: 'Image: /b.png' }, photo, inline] }]
			},
			{ role: 'assistant', content: [{ type: 'text', text: 'Two files.' }] }
		]);
	});

	it("sends another provider the text and calls of replies, and notes for files it can't open", () => {
		const { chat } = claudeChat();
		const gpt = setPreset(chat.id, makePreset('GPT', 'gpt-6-astra', 'openai').id);
		expect(toResponsesInput(requestMessages(committedRows(chat.id), null), gpt.model)).toEqual([
			{
				role: 'user',
				content: [
					{ type: 'input_text', text: '[Anna attached a.png, saved at /a.png]' },
					{ type: 'input_text', text: note }
				]
			},
			{ role: 'assistant', content: 'Looking.' },
			{
				type: 'function_call',
				call_id: 't1',
				name: 'run_command',
				arguments: '{"command":"ls"}'
			},
			{
				type: 'function_call_output',
				call_id: 't1',
				output: [
					{ type: 'input_text', text: 'Image: /b.png' },
					{ type: 'input_text', text: note },
					{
						type: 'input_image',
						image_url: 'data:image/png;base64,iVBORw0KGgo=',
						detail: 'auto'
					}
				]
			},
			{ role: 'assistant', content: 'Two files.' }
		]);
	});

	it("knows which providers hold files another can't open", () => {
		const { chat } = claudeChat();
		expect(heldFileProviders(chat.id)).toEqual(['anthropic']);
		// Kept by btw, a picture goes to any provider.
		const { user, profile } = makeFamily('Max');
		const newer = createConversation({
			profile,
			presetId: makePreset('Opus', 'claude-opus-5-5').id,
			userId: user.id
		});
		insertQueued({
			conversationId: newer.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Look',
			attachments: {
				content: [
					{ type: 'image', source: { type: 'media', sha256: 'ab', mime: 'image/png', bytes: 2 } }
				],
				files: [],
				media: [],
				uploadIds: []
			}
		});
		commitQueuedRows(newer.id);
		expect(heldFileProviders(newer.id)).toEqual([]);
	});

	it('sends Claude what OpenAI wrote as text and calls', () => {
		const { user, profile } = makeFamily();
		const gpt = makePreset('GPT', 'gpt-6-astra', 'openai');
		const chat = createConversation({ profile, presetId: gpt.id, userId: user.id });
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Files?'
		});
		commitQueuedRows(chat.id);
		answer(chat, [
			{ id: 'rs_1', type: 'reasoning', summary: [], encrypted_content: 'gAAAA' },
			{
				id: 'msg_1',
				type: 'message',
				role: 'assistant',
				content: [{ type: 'output_text', text: 'Looking.', annotations: [] }]
			},
			{
				id: 'fc_1',
				type: 'function_call',
				call_id: 'call_1',
				name: 'run_command',
				arguments: '{"command":"ls"}'
			},
			{ id: 'fc_2', type: 'function_call', call_id: 'call_2', name: 'run_command', arguments: '{' }
		]);
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([
				{ type: 'tool_result', tool_use_id: 'call_1', content: 'a.txt' },
				{ type: 'tool_result', tool_use_id: 'call_2', content: 'Invalid input', is_error: true }
			]),
			provider: 'openai'
		});
		setPreset(chat.id, makePreset().id);
		expect(forClaude(committedRows(chat.id))[1]).toEqual({
			role: 'assistant',
			content: [
				{ type: 'text', text: 'Looking.' },
				{ type: 'tool_use', id: 'call_1', name: 'run_command', input: { command: 'ls' } },
				{ type: 'tool_use', id: 'call_2', name: 'run_command', input: {} }
			]
		});
	});

	it("sends OpenAI's reasoning back only to the model that wrote it", () => {
		const { user, profile } = makeFamily();
		const gpt = makePreset('GPT', 'gpt-6-astra', 'openai');
		const chat = createConversation({ profile, presetId: gpt.id, userId: user.id });
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		commitQueuedRows(chat.id);
		const reasoning = { id: 'rs_1', type: 'reasoning', summary: [], encrypted_content: 'gAAAA' };
		const hello = {
			id: 'msg_1',
			type: 'message',
			role: 'assistant',
			content: [{ type: 'output_text', text: 'Hello!', annotations: [] }]
		};
		answer(chat, [reasoning, hello]);

		const rows = committedRows(chat.id);
		expect(toResponsesInput(requestMessages(rows, null), 'gpt-6-mini')[1]).toEqual({
			role: 'assistant',
			content: 'Hello!'
		});
		expect(toResponsesInput(requestMessages(rows, null), 'gpt-6-astra').slice(1)).toEqual([
			reasoning,
			hello
		]);
	});

	it("leaves out the thinking a Claude plan's Claude Code got, which belongs to another account", () => {
		const { user, profile } = makeFamily();
		const plan = makePreset('Plan', 'claude-opus-5-5', 'claude-plan');
		const chat = createConversation({ profile, presetId: plan.id, userId: user.id });
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		commitQueuedRows(chat.id);
		answer(chat, [thinking, { type: 'text', text: 'Hello!' }]);
		setPreset(chat.id, makePreset().id);
		expect(forClaude(committedRows(chat.id))[1]).toEqual({
			role: 'assistant',
			content: [{ type: 'text', text: 'Hello!' }]
		});
	});
});

describe('changing conversations', () => {
	it('sets a new title only if nobody changed it meanwhile', () => {
		const { chat } = newChat({ title: 'New chat' });
		expect(replaceTitle(chat.id, 'New chat', 'Weather')).toBe(true);
		expect(replaceTitle(chat.id, 'New chat', 'Something else')).toBe(false);
		expect(getConversation(chat.id)?.title).toBe('Weather');
	});

	it('stops the background run of a conversation it deletes', () => {
		const { profile, chat } = newChat({ hidden: true });
		const run = queueWake({ profileId: profile.id, text: 'Check the mail' });
		updateRun(run.id, { status: 'running', conversationId: chat.id });

		deleteConversation(chat.id);
		expect(getConversation(chat.id)).toBeUndefined();
		expect(getRun(run.id)).toMatchObject({ status: 'stopped', conversationId: null });
		expect(getRun(run.id)?.finishedAt).toBeInstanceOf(Date);
	});

	it('deletes background runs nobody continued once they are old', () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const { user, profile, preset, chat: old } = newChat({ hidden: true });
		const visible = createConversation({ profile, presetId: preset.id, userId: user.id });
		vi.setSystemTime(Date.UTC(2026, 8, 10));
		const recent = createConversation({
			profile,
			presetId: preset.id,
			userId: user.id,
			hidden: true
		});

		deleteHiddenConversations(new Date(Date.UTC(2026, 8, 5)));
		expect(getConversation(old.id)).toBeUndefined();
		expect(getConversation(visible.id)).toBeDefined();
		expect(getConversation(recent.id)).toBeDefined();
	});
});
