import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { streamTurn } from './anthropic.ts';
import {
	committedRows,
	createConversation,
	insertQueued,
	type DisplayMessage
} from './conversations.ts';
import { viewImage } from './images.ts';
import { getMedia, mediaFile } from './media.ts';
import { paths } from './paths.ts';
import { providerFileId } from './provider-files.ts';
import { runCommand, type RunCommandResult } from './run-command.ts';
import { getSnapshot, kick, onLoopEnd, recoverAfterRestart, subscribe } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	streamTurn: vi.fn()
}));

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

vi.mock('./provider-files.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./provider-files.ts')>()),
	providerFileId: vi.fn()
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

/** A 1×1 PNG, which `btw view` sends as it is. */
const DOT = Buffer.from(
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
	'base64'
);

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

	it('keeps what a command looked at with `btw view`, for the chat to show with that command', async () => {
		const look = {
			type: 'tool_use',
			id: 't2',
			name: 'run_command',
			input: { summary: 'Looking at the dot', command: 'btw view dot.png' }
		};
		const chat = chatAsking(
			modelReply([listFiles, look], 'tool_use'),
			modelReply([{ type: 'text', text: 'A dot.' }], 'end_turn')
		);
		const picture = join(paths.home, 'dot.png');
		writeFileSync(picture, DOT);
		vi.mocked(providerFileId).mockResolvedValue('file_dot');
		vi.mocked(runCommand).mockImplementation(async (input, { env }) => {
			if (input.command === 'ls') return { content: 'a.txt', isError: false, exitCode: 0 };
			const line = await viewImage(picture, env.BTW_VIEW_DIR!);
			return { content: line, isError: false, exitCode: 0 };
		});
		const sent: DisplayMessage[] = [];
		subscribe(chat.id, (event) => {
			if (event.type === 'message' && event.message.kind === 'tool_results') {
				sent.push(event.message);
			}
		});

		await run(chat.id);

		// The model gets the picture itself; the chat gets btw's copy of it.
		expect(results(chat.id)).toEqual([
			[
				{ type: 'tool_result', tool_use_id: 't1', content: 'a.txt' },
				{
					type: 'tool_result',
					tool_use_id: 't2',
					content: [
						{ type: 'text', text: `Attached ${picture} (1×1 PNG).` },
						{ type: 'text', text: `Image: ${picture}` },
						{ type: 'image', source: { type: 'file', file_id: 'file_dot' } }
					]
				}
			]
		]);
		const saved = getSnapshot(chat.id).messages.find((m) => m.kind === 'tool_results');
		expect(sent).toEqual([saved]);
		expect(saved?.kind === 'tool_results' && saved.results.map((r) => r.images)).toEqual([
			[],
			[
				{
					status: 'ok',
					id: expect.any(String),
					name: 'dot.png',
					mime: 'image/png',
					bytes: DOT.length,
					width: 1,
					height: 1,
					viewable: true,
					path: picture
				}
			]
		]);
		const [shown] = saved?.kind === 'tool_results' ? saved.results[1].images : [];
		const file = mediaFile(getMedia(chat.id, shown.id)!, 'view');
		expect(readFileSync(file!.path)).toEqual(DOT);
	});
});
