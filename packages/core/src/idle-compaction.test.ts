import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { streamTurn, toAnthropicMessages } from './anthropic.ts';
import { summaryRequest } from './compaction.ts';
import { initConfig, updateConfig } from './config.ts';
import { committedRows, createConversation, insertQueued } from './conversations.ts';
import { startIdleCompaction } from './idle-compaction.ts';
import { getSnapshot, kick, onLoopEnd, onRunningChange } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	streamTurn: vi.fn()
}));

const MINUTE = 60_000;

function reply(text: string, cacheRead = 0): Anthropic.Message {
	return {
		id: 'msg',
		type: 'message',
		role: 'assistant',
		model: 'claude-sonnet-5',
		content: [{ type: 'text', text }],
		stop_reason: 'end_turn',
		stop_sequence: null,
		usage: { input_tokens: 1_000, cache_read_input_tokens: cacheRead, output_tokens: 50 }
	} as unknown as Anthropic.Message;
}

/** A chat with a reply the size of `cacheRead` and then some, once its turn is over. */
async function answeredChat(cacheRead: number, name = 'Anna') {
	const { user, profile } = makeFamily(name);
	const preset = makePreset(`Sonnet for ${name}`);
	const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
	insertQueued({ conversationId: chat.id, senderId: user.id, senderName: name, text: 'Hi' });
	vi.mocked(streamTurn).mockResolvedValueOnce(reply('Hello.', cacheRead));
	await turn(chat.id);
	return { chat, user };
}

function turn(conversationId: string): Promise<void> {
	const ended = new Promise<void>((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
	kick(conversationId);
	return ended;
}

function stopped(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onRunningChange((id, running) => {
			if (id !== conversationId || running) return;
			off();
			resolve();
		});
	});
}

beforeEach(() => {
	vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] });
	initConfig();
	updateConfig((c) => {
		c.commandMode = 'unrestricted';
		c.compactWhenIdle = 30;
	});
	startIdleCompaction();
});

afterEach(() => {
	vi.useRealTimers();
	vi.resetAllMocks();
});

describe('quiet chats', () => {
	it('are summarized once quiet for the minutes set', async () => {
		const { chat } = await answeredChat(30_000);
		await vi.advanceTimersByTimeAsync(29 * MINUTE);
		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(1);

		vi.mocked(streamTurn).mockResolvedValueOnce(reply('<summary>Anna said hi.</summary>'));
		const done = stopped(chat.id);
		await vi.advanceTimersByTimeAsync(MINUTE);
		await done;
		const ask = vi.mocked(streamTurn).mock.calls[1][0];
		expect(toAnthropicMessages(ask.messages).at(-1)).toEqual({
			role: 'user',
			content: [{ type: 'text', text: summaryRequest('idle') }]
		});
		expect(committedRows(chat.id).at(-1)).toMatchObject({
			kind: 'compaction',
			senderName: null,
			text: 'Anna said hi.'
		});

		// Summarized: nothing new for another one.
		await vi.advanceTimersByTimeAsync(60 * MINUTE);
		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(2);
	});

	it('count again from a new message', async () => {
		const { chat, user } = await answeredChat(30_000);
		await vi.advanceTimersByTimeAsync(20 * MINUTE);
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hm?' });
		vi.mocked(streamTurn).mockResolvedValueOnce(reply('Yes?', 31_000));
		await turn(chat.id);
		await vi.advanceTimersByTimeAsync(20 * MINUTE);
		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(2);

		vi.mocked(streamTurn).mockResolvedValueOnce(reply('<summary>Anna asked twice.</summary>'));
		const done = stopped(chat.id);
		await vi.advanceTimersByTimeAsync(10 * MINUTE);
		await done;
		expect(committedRows(chat.id).at(-1)?.kind).toBe('compaction');
	});

	it('leave small chats alone, and say nothing in the chat when the summary fails', async () => {
		const small = (await answeredChat(5_000)).chat;
		await vi.advanceTimersByTimeAsync(31 * MINUTE);
		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(1);
		expect(committedRows(small.id).at(-1)?.kind).toBe('assistant');

		const { chat } = await answeredChat(30_000, 'Boris');
		vi.mocked(streamTurn).mockRejectedValueOnce(new Error('Overloaded'));
		const done = stopped(chat.id);
		await vi.advanceTimersByTimeAsync(30 * MINUTE);
		await done;
		expect(getSnapshot(chat.id)).toMatchObject({ running: false, error: null });
		expect(committedRows(chat.id).at(-1)?.kind).toBe('assistant');
	});
});
