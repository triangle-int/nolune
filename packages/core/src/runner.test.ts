import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { streamTurn, toAnthropicMessages } from './anthropic.ts';
import { eq } from 'drizzle-orm';
import { stopBackgroundCommands } from './background.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	insertQueued
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { conversation } from './db/schema.ts';
import { LEGACY_TOOLS, TOOLS, runCommand, type RunCommandResult } from './run-command.ts';
import {
	changeEffort,
	changeModel,
	getSnapshot,
	kick,
	onLoopEnd,
	onRunningChange,
	recoverAfterRestart,
	runningConversationIds,
	subscribe,
	type LiveEvent
} from './runner.ts';
import { runSubagent, setSubagentStatus } from './subagents.ts';
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

/** Resolves when the conversation's agent loop next stops. */
function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

/** Starts the agent and resolves when its loop stops. */
function run(conversationId: string): Promise<void> {
	const ended = loopEnd(conversationId);
	kick(conversationId);
	return ended;
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
			[{ type: 'tool_result', callId: 't1', content: 'a.txt', isError: false }]
		]);
		expect(toAnthropicMessages(vi.mocked(streamTurn).mock.calls[1][0].messages).at(-1)).toEqual({
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
					callId: 't1',
					content: 'Not finished: spawn EAGAIN',
					isError: true
				}
			]
		]);
		expect(streamTurn).toHaveBeenCalledTimes(2);
		expect(committedRows(chat.id).at(-1)?.kind).toBe('assistant');
		expect(logged).toHaveBeenCalled();
	});

	it('tells listeners when a chat starts and stops working, for the sidebar', async () => {
		const chat = chatAsking(
			modelReply([listFiles], 'tool_use'),
			modelReply([{ type: 'text', text: 'One file.' }], 'end_turn')
		);
		let finish!: (result: RunCommandResult) => void;
		vi.mocked(runCommand).mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));
		const changes: [string, boolean][] = [];
		const off = onRunningChange((id, running) => changes.push([id, running]));

		const ended = run(chat.id);
		await vi.waitFor(() => expect(runCommand).toHaveBeenCalled());
		expect(runningConversationIds()).toContain(chat.id);
		finish({ content: 'a.txt', isError: false, exitCode: 0 });
		await ended;
		off();

		expect(changes).toEqual([
			[chat.id, true],
			[chat.id, false]
		]);
		expect(runningConversationIds()).not.toContain(chat.id);
	});
});

describe('tools and cache', () => {
	it('sends a chat the tools it was created with, and an hour-long cache', async () => {
		const chat = chatAsking(modelReply([{ type: 'text', text: 'Hi.' }], 'end_turn'));
		await run(chat.id);
		expect(vi.mocked(streamTurn).mock.calls[0][0]).toMatchObject({ tools: TOOLS, cacheTtl: '1h' });
	});

	it('keeps the first run_command for chats from before tools were saved', async () => {
		const chat = chatAsking(modelReply([{ type: 'text', text: 'Hi.' }], 'end_turn'));
		getDb().update(conversation).set({ tools: null }).where(eq(conversation.id, chat.id)).run();
		await run(chat.id);
		expect(vi.mocked(streamTurn).mock.calls[0][0].tools).toBe(LEGACY_TOOLS);
	});
});

describe('switching models', () => {
	it('goes on with the new model, even in the middle of a turn, and tells everyone watching', async () => {
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
		// GPT asked for a command, and got its result, before the chat switched.
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([
				{ id: 'rs_1', type: 'reasoning', summary: [], encrypted_content: 'gAAAA' },
				{
					id: 'fc_1',
					type: 'function_call',
					call_id: 'call_1',
					name: 'run_command',
					arguments: '{"command":"ls"}'
				}
			]),
			provider: 'openai',
			model: 'gpt-6-astra'
		});
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([{ type: 'tool_result', tool_use_id: 'call_1', content: 'a.txt' }]),
			provider: 'openai'
		});
		const events: LiveEvent[] = [];
		const off = subscribe(chat.id, (event) => events.push(event));
		const sonnet = makePreset();
		changeModel(chat.id, sonnet.id);
		changeEffort(chat.id, 'high');
		off();
		const model = {
			presetId: sonnet.id,
			presetName: 'Sonnet',
			provider: 'anthropic',
			contextWindow: 200_000
		};
		expect(events).toEqual([
			{ type: 'model', model: { ...model, effort: 'medium' } },
			{ type: 'model', model: { ...model, effort: 'high' } }
		]);
		expect(getSnapshot(chat.id).model).toEqual({ ...model, effort: 'high' });

		vi.mocked(streamTurn).mockResolvedValueOnce(
			modelReply([{ type: 'text', text: 'One file: a.txt.' }], 'end_turn')
		);
		await run(chat.id);
		const request = vi.mocked(streamTurn).mock.calls[0][0];
		expect(request).toMatchObject({ model: 'claude-sonnet-5', effort: 'high' });
		expect(toAnthropicMessages(request.messages)).toEqual([
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Files?' }] },
			{
				role: 'assistant',
				content: [{ type: 'tool_use', id: 'call_1', name: 'run_command', input: { command: 'ls' } }]
			},
			{ role: 'user', content: [{ type: 'tool_result', tool_use_id: 'call_1', content: 'a.txt' }] }
		]);
		expect(committedRows(chat.id).at(-1)).toMatchObject({
			kind: 'assistant',
			provider: 'anthropic',
			model: 'claude-sonnet-5'
		});
	});
});

describe('background commands', () => {
	const download = {
		type: 'tool_use',
		id: 'bg1',
		name: 'run_command',
		input: { summary: 'Downloading the photos', command: 'fetch-photos', run_in_background: true }
	};

	it('answers at once, then hands the output over as a message when the command ends', async () => {
		const chat = chatAsking(
			modelReply([download], 'tool_use'),
			modelReply([{ type: 'text', text: "I'll tell you when it's done." }], 'end_turn'),
			modelReply([{ type: 'text', text: 'All 120 photos are in.' }], 'end_turn')
		);
		let finish!: (result: RunCommandResult) => void;
		vi.mocked(runCommand).mockImplementationOnce((_input, options) => {
			options.onStart?.(4242);
			return new Promise((resolve) => (finish = resolve));
		});

		await run(chat.id);
		expect(vi.mocked(runCommand).mock.calls[0][0]).toMatchObject({ background: true });
		const [[started]] = results(chat.id);
		expect(started).toMatchObject({ callId: 'bg1', isError: false });
		expect(started.content).toContain('Started in the background (process group 4242)');

		// The output starts the agent again.
		const second = loopEnd(chat.id);
		finish({ content: 'saved 120 photos\n[exit code 0]', isError: false, exitCode: 0 });
		await second;

		const notice = committedRows(chat.id).find((row) => row.kind === 'task_result')!;
		expect(notice).toMatchObject({ senderName: 'Downloading the photos' });
		const told = toAnthropicMessages(vi.mocked(streamTurn).mock.calls[2][0].messages).at(-1);
		expect(JSON.stringify(told)).toContain(
			'[Background command finished: Downloading the photos]\\n$ fetch-photos\\nsaved 120 photos'
		);
		expect(committedRows(chat.id).at(-1)?.kind).toBe('assistant');
	});

	it('lists a background `nolune agent watch` as the subagent it waits for, not twice', async () => {
		const background = (id: string, summary: string, command: string) => ({
			type: 'tool_use',
			id,
			name: 'run_command',
			input: { summary, command, run_in_background: true }
		});
		const chat = chatAsking(
			modelReply(
				[
					background('w1', 'Waiting for the flight search', 'nolune agent watch flights'),
					background('w2', 'Waiting for a subagent nobody started', 'nolune agent watch nobody'),
					background('d1', 'Downloading the photos', 'fetch-photos')
				],
				'tool_use'
			),
			modelReply([{ type: 'text', text: "I'll tell you when they're done." }], 'end_turn')
		);
		const { subagent } = runSubagent({
			parentId: chat.id,
			name: 'flights',
			prompt: 'Find flights.'
		});
		const finishers: ((result: RunCommandResult) => void)[] = [];
		vi.mocked(runCommand).mockImplementation((_input, options) => {
			options.onStart?.(4242);
			return new Promise((resolve) => finishers.push(resolve));
		});

		await run(chat.id);
		const listed = () =>
			getSnapshot(chat.id).background.map((item) =>
				item.kind === 'subagent' ? `subagent ${item.name}` : item.summary
			);
		expect(listed()).toEqual([
			'Waiting for a subagent nobody started',
			'Downloading the photos',
			'subagent flights'
		]);

		// Done, while its watch hasn't noticed yet: nothing flashes back in its place.
		setSubagentStatus(subagent.id, 'done');
		expect(listed()).toEqual(['Waiting for a subagent nobody started', 'Downloading the photos']);

		stopBackgroundCommands(chat.id, 'Anna');
		for (const finish of finishers) finish({ content: '', isError: true, exitCode: null });
	});
});
