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
	insertQueued,
	lastCommittedRow,
	listConversations,
	queuedRows,
	replaceTitle,
	replyText,
	setHidden,
	toDisplay,
	touchConversation
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { upload } from './db/schema.ts';
import { removePreset } from './presets.ts';
import { makeFamily, makePreset, makeUser } from './test/fixtures.ts';
import { getRun, queueWake, updateRun } from './triggers.ts';

afterEach(() => {
	vi.useRealTimers();
});

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
