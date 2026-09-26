import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { streamTurn } from './anthropic.ts';
import { committedRows, createConversation, insertQueued } from './conversations.ts';
import { runCommand, type RunCommandResult } from './run-command.ts';
import { kick, onLoopEnd, recoverAfterRestart } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	streamTurn: vi.fn()
}));

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

afterEach(() => {
	vi.resetAllMocks();
});

function modelReply(content: unknown[], stopReason: Anthropic.StopReason): Anthropic.Message {
	return {
		id: 'msg',
		type: 'message',
		role: 'assistant',
		model: 'claude-sonnet-5',
		content,
		stop_reason: stopReason,
		stop_sequence: null,
		usage: { input_tokens: 10, output_tokens: 5 }
	} as unknown as Anthropic.Message;
}

const listFiles = { type: 'tool_use', id: 't1', name: 'run_command', input: { command: 'ls' } };

/** A chat where Anna asked for something, with the model's replies lined up. */
function chatAsking(...replies: Anthropic.Message[]) {
	const { user, profile } = makeFamily();
	const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
	insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Files?' });
	for (const reply of replies) vi.mocked(streamTurn).mockResolvedValueOnce(reply);
	return chat;
}

/** Starts the agent and resolves when its loop stops. */
function run(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
		kick(conversationId);
	});
}

function results(conversationId: string) {
	return committedRows(conversationId)
		.filter((row) => row.kind === 'tool_results')
		.map((row) => JSON.parse(row.content));
}

describe('the agent loop', () => {
	it('keeps a command that is still running when recovery runs again, as `pnpm dev` does', async () => {
		const chat = chatAsking(
			modelReply([listFiles], 'tool_use'),
			modelReply([{ type: 'text', text: 'One file.' }], 'end_turn')
		);
		let finish!: (result: RunCommandResult) => void;
		vi.mocked(runCommand).mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));

		const ended = run(chat.id);
		await vi.waitFor(() => expect(runCommand).toHaveBeenCalled());
		recoverAfterRestart();
		finish({ content: 'a.txt', isError: false, exitCode: 0 });
		await ended;

		expect(results(chat.id)).toEqual([
			[{ type: 'tool_result', tool_use_id: 't1', content: 'a.txt' }]
		]);
		expect(vi.mocked(streamTurn).mock.calls[1][0].messages.at(-1)).toEqual({
			role: 'user',
			content: [{ type: 'tool_result', tool_use_id: 't1', content: 'a.txt' }]
		});
	});

	it('answers a call whose command throws, and carries on', async () => {
		const chat = chatAsking(
			modelReply([listFiles], 'tool_use'),
			modelReply([{ type: 'text', text: 'That failed.' }], 'end_turn')
		);
		vi.mocked(runCommand).mockRejectedValueOnce(new Error('spawn EAGAIN'));
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});

		await run(chat.id);

		expect(results(chat.id)).toEqual([
			[
				{
					type: 'tool_result',
					tool_use_id: 't1',
					content: 'Not finished: spawn EAGAIN',
					is_error: true
				}
			]
		]);
		expect(streamTurn).toHaveBeenCalledTimes(2);
		expect(committedRows(chat.id).at(-1)?.kind).toBe('assistant');
		expect(logged).toHaveBeenCalled();
	});
});
