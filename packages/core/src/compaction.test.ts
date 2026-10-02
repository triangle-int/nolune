import { describe, expect, it } from 'vitest';
import { supportsCompaction } from './anthropic.ts';
import {
	idleCompactable,
	needsCompaction,
	saveIdleCompaction,
	serverCompactAt,
	summaryOf
} from './compaction.ts';
import { initConfig, readConfig } from './config.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	insertQueued,
	type Conversation,
	type Usage
} from './conversations.ts';
import { summarizeAnthropicUsage } from './models.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

describe('compacting on the server', () => {
	it('is for every Claude model since Opus and Sonnet 4.6, and new ones', () => {
		for (const model of [
			'claude-opus-5-5',
			'claude-sonnet-5-5',
			'claude-fable-5-1',
			'claude-mythos-5',
			'claude-opus-4-8',
			'claude-opus-4-6',
			'claude-sonnet-4-6',
			'claude-opus-6'
		]) {
			expect(supportsCompaction(model), model).toBe(true);
		}
		for (const model of [
			'claude-haiku-4-5',
			'claude-haiku-4-5-20251001',
			'claude-opus-4-5',
			'claude-opus-4-5-20251101',
			'claude-sonnet-4-5-20250929',
			'claude-opus-4-1',
			'claude-sonnet-4-0',
			'claude-opus-4-20250514',
			'claude-3-7-sonnet-latest'
		]) {
			expect(supportsCompaction(model), model).toBe(false);
		}
	});

	it('starts at 85% of the window, and never under the 50,000 tokens Claude takes', () => {
		expect(serverCompactAt(1_000_000)).toBe(850_000);
		expect(serverCompactAt(null)).toBe(170_000);
		expect(serverCompactAt(32_000)).toBe(50_000);
	});

	it("counts the summary's tokens apart from the reply's", () => {
		const usage = summarizeAnthropicUsage({
			input_tokens: 2_000,
			output_tokens: 300,
			cache_read_input_tokens: 0,
			cache_creation_input_tokens: 1_000,
			iterations: [
				{
					type: 'compaction',
					input_tokens: 10_000,
					output_tokens: 1_500,
					cache_read_input_tokens: 170_000,
					cache_creation_input_tokens: 0,
					cache_creation: null
				},
				{
					type: 'message',
					input_tokens: 2_000,
					output_tokens: 300,
					cache_read_input_tokens: 0,
					cache_creation_input_tokens: 1_000,
					cache_creation: null,
					model: 'claude-opus-5-5'
				}
			]
		} as never);
		expect(usage).toEqual({
			input: 2_000,
			cacheRead: 0,
			cacheWrite: 1_000,
			output: 300,
			compaction: { input: 10_000, cacheRead: 170_000, cacheWrite: 0, output: 1_500 }
		});
		expect(
			summarizeAnthropicUsage({ input_tokens: 5, output_tokens: 1 } as never)
		).not.toHaveProperty('compaction');
	});
});

describe('compacting in the runner', () => {
	/** A chat on `model` (`provider`) whose window is 200,000 tokens, with a reply that used `used`. */
	function chatThatUsed(used: number, model = 'gpt-6-astra', provider = 'openai' as const) {
		const { user, profile } = makeFamily();
		const preset = makePreset('Model', model, provider);
		const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		commitQueuedRows(chat.id);
		reply(chat, used);
		return chat;
	}

	function reply(chat: Conversation, used: number) {
		const usage: Usage = { input: 1_000, cacheRead: used - 1_100, cacheWrite: 0, output: 100 };
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'Hello.' }]),
			usage
		});
	}

	it('summarizes once the latest call filled 85% of the window', () => {
		const chat = chatThatUsed(169_999);
		expect(needsCompaction(chat, committedRows(chat.id))).toBe(false);
		reply(chat, 170_000);
		expect(needsCompaction(chat, committedRows(chat.id))).toBe(true);
	});

	it('leaves a window nobody knows, Claude, and the Claude plan alone', () => {
		const chat = chatThatUsed(190_000);
		const rows = committedRows(chat.id);
		expect(needsCompaction({ ...chat, contextWindow: null }, rows)).toBe(false);
		expect(
			needsCompaction({ ...chat, provider: 'anthropic', model: 'claude-opus-5-5' }, rows)
		).toBe(false);
		expect(needsCompaction({ ...chat, provider: 'claude-plan', model: 'opus' }, rows)).toBe(false);
		// Haiku 4.5 can't compact on the server, so the runner does.
		expect(
			needsCompaction({ ...chat, provider: 'anthropic', model: 'claude-haiku-4-5' }, rows)
		).toBe(true);
	});

	it('waits for a call after the latest summary, and counts from there', () => {
		const chat = chatThatUsed(190_000);
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'compaction',
			text: 'Anna said hi.',
			blocks: [{ type: 'text', text: 'Anna said hi.' }]
		});
		// A window the summary itself overflows would get one summary after another.
		reply(chat, 180_000);
		expect(needsCompaction(chat, committedRows(chat.id))).toBe(false);
		reply(chat, 150_000);
		expect(needsCompaction(chat, committedRows(chat.id))).toBe(false);
		reply(chat, 175_000);
		expect(needsCompaction(chat, committedRows(chat.id))).toBe(true);
	});

	it('takes the summary from its tags, or the whole reply without them', () => {
		const ask = (texts: string[], stopReason = 'end_turn') => summaryOf({ texts, stopReason });
		expect(ask(['Here it is.\n<summary>\nAnna wants the files.\n</summary>'])).toBe(
			'Anna wants the files.'
		);
		expect(ask(['Anna wants the files.'])).toBe('Anna wants the files.');
		expect(ask(['<summary>Anna wants'], 'max_tokens')).toBeNull();
		expect(ask(['  '])).toBeNull();
		expect(ask([], 'refusal')).toBeNull();
		// It ran a command instead: what it said first is no summary.
		expect(ask(['Let me check the files first.'], 'tool_use')).toBeNull();
		expect(ask(['<summary>Anna wants the files.</summary>'], 'tool_use')).toBe(
			'Anna wants the files.'
		);
	});
});

describe('summarizing quiet chats', () => {
	function quietChat(used: number) {
		const { user, profile } = makeFamily();
		const preset = makePreset('GPT', 'gpt-6-astra', 'openai');
		const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		commitQueuedRows(chat.id);
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'Hello.' }]),
			usage: { input: 1_000, cacheRead: used - 1_100, cacheWrite: 0, output: 100 }
		});
		return { chat, user };
	}

	it('is for a chat that is big enough, answered, and new since its latest summary', () => {
		const { chat, user } = quietChat(25_000);
		expect(idleCompactable(chat, committedRows(chat.id))).toBe(true);
		expect(idleCompactable({ ...chat, hidden: true }, committedRows(chat.id))).toBe(false);
		expect(idleCompactable({ ...chat, provider: 'claude-plan' }, committedRows(chat.id))).toBe(
			false
		);

		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'And?' });
		commitQueuedRows(chat.id);
		// It waits on an answer.
		expect(idleCompactable(chat, committedRows(chat.id))).toBe(false);
	});

	it('leaves small chats and summarized ones alone', () => {
		const small = quietChat(19_000).chat;
		expect(idleCompactable(small, committedRows(small.id))).toBe(false);
	});

	it('takes whole minutes up to a week, or off', () => {
		initConfig();
		saveIdleCompaction(55);
		expect(readConfig().compactWhenIdle).toBe(55);
		for (const wrong of [0, 1.5, 7 * 24 * 60 + 1, Number.NaN]) {
			expect(() => saveIdleCompaction(wrong)).toThrow('whole number from 1 to 10080');
		}
		saveIdleCompaction(null);
		expect(readConfig().compactWhenIdle).toBeUndefined();
	});
});
