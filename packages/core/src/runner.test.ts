import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createMessage, streamTurn, toAnthropicMessages } from './anthropic.ts';
import { eq } from 'drizzle-orm';
import { stopBackgroundCommands } from './background.ts';
import { SAFETY_STOP, saveCommandMode } from './command-safety.ts';
import { summaryRequest } from './compaction.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	insertQueued
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { addMemoryFact } from './memory.ts';
import { conversation } from './db/schema.ts';
import { LEGACY_TOOLS, TOOLS, runCommand, type RunCommandResult } from './run-command.ts';
import {
	CompactionError,
	changeCommandMode,
	changeEffort,
	changeModel,
	compactConversation,
	getSnapshot,
	kick,
	onLoopEnd,
	onRunningChange,
	recoverAfterRestart,
	runningConversationIds,
	sendMessage,
	setTyping,
	stop,
	subscribe,
	TYPING_TTL_MS,
	type LiveEvent
} from './runner.ts';
import { runSubagent, setSubagentStatus } from './subagents.ts';
import { makeFamily, makePreset, makeUser, runCommandsUnchecked } from './test/fixtures.ts';

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	streamTurn: vi.fn(),
	// Auto mode's checks, on the chat's own model.
	createMessage: vi.fn()
}));

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

beforeEach(() => {
	// Auto mode has tests of its own, at the end.
	runCommandsUnchecked();
});

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

describe('memory', () => {
	it('goes along with a message for the model only, once per chat', async () => {
		const { user, profile } = makeFamily();
		addMemoryFact(profile.slug, 'home', 'Wifi password: mango42');
		const chat = createConversation({
			profile,
			presetId: makePreset().id,
			userId: user.id,
			title: 'Wifi'
		});
		vi.mocked(streamTurn).mockResolvedValue(
			modelReply([{ type: 'text', text: 'mango42' }], 'end_turn')
		);
		for (const text of ["What's the wifi password?", 'Is the wifi password still the same?']) {
			const ended = loopEnd(chat.id);
			await sendMessage(chat.id, user, text);
			await ended;
		}

		const [first, second] = committedRows(chat.id).filter((row) => row.kind === 'human');
		expect(JSON.parse(first.content)).toEqual([
			{ type: 'text', text: "Anna: What's the wifi password?" },
			{ type: 'text', text: expect.stringContaining('- [home] Wifi password: mango42') }
		]);
		expect(JSON.parse(second.content)).toEqual([
			{ type: 'text', text: 'Anna: Is the wifi password still the same?' }
		]);
		expect(getSnapshot(chat.id).messages[0]).toMatchObject({
			kind: 'human',
			text: "What's the wifi password?"
		});
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

describe('compaction', () => {
	const summary = {
		type: 'compaction',
		content: 'Anna asked for the files.',
		encrypted_content: null
	};
	const listed = { type: 'tool_result', tool_use_id: 't1', content: 'a.txt' };

	beforeEach(() => {
		vi.mocked(runCommand).mockResolvedValue({ content: 'a.txt', isError: false, exitCode: 0 });
	});

	it('has Claude summarize on the server at 85% of the window, and goes on from there', async () => {
		const chat = chatAsking(
			modelReply([summary, listFiles], 'tool_use'),
			modelReply([{ type: 'text', text: 'One file.' }], 'end_turn')
		);
		await run(chat.id);

		const [first, second] = vi.mocked(streamTurn).mock.calls.map(([request]) => request);
		expect(first.compactAt).toBe(170_000);
		expect(toAnthropicMessages(second.messages)).toEqual([
			{ role: 'assistant', content: [summary, listFiles] },
			{ role: 'user', content: [listed] }
		]);
		expect(getSnapshot(chat.id).messages[1]).toMatchObject({
			kind: 'assistant',
			blocks: [{ type: 'compaction', summary: 'Anna asked for the files.' }, { type: 'tool' }]
		});
	});

	/** A chat on Haiku 4.5, which can't compact on the server, whose first step filled its window. */
	function haikuChatAsking(...replies: Anthropic.Message[]) {
		const { user, profile } = makeFamily();
		const preset = makePreset('Haiku', 'claude-haiku-4-5');
		const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Files?'
		});
		const full = modelReply([listFiles], 'tool_use');
		full.usage = { ...full.usage, input_tokens: 1_000, cache_read_input_tokens: 175_000 };
		for (const reply of [full, ...replies]) vi.mocked(streamTurn).mockResolvedValueOnce(reply);
		return chat;
	}

	it('has the chat model summarize before a call that would outgrow its window', async () => {
		const chat = haikuChatAsking(
			modelReply(
				[
					{
						type: 'text',
						text: 'Sure.\n<summary>Anna asked for the files; ls found a.txt.</summary>'
					}
				],
				'end_turn'
			),
			modelReply([{ type: 'text', text: 'One file: a.txt.' }], 'end_turn')
		);
		const events: LiveEvent[] = [];
		const off = subscribe(chat.id, (event) => events.push(event));
		await run(chat.id);
		off();

		const [first, ask, next] = vi.mocked(streamTurn).mock.calls.map(([request]) => request);
		expect(first.compactAt).toBeNull();
		expect(ask.compactAt).toBeUndefined();
		// The conversation's next request, with the ask after it: the cache holds the rest.
		expect(toAnthropicMessages(ask.messages)).toEqual([
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Files?' }] },
			{ role: 'assistant', content: [listFiles] },
			{ role: 'user', content: [listed] },
			{ role: 'user', content: [{ type: 'text', text: summaryRequest('window') }] }
		]);
		const note =
			'[Earlier in this conversation, summarized to fit the context window:]\n\nAnna asked for the files; ls found a.txt.';
		expect(toAnthropicMessages(next.messages)).toEqual([
			{ role: 'user', content: [{ type: 'text', text: note }] }
		]);
		expect(committedRows(chat.id).map((row) => row.kind)).toEqual([
			'human',
			'assistant',
			'tool_results',
			'compaction',
			'assistant'
		]);
		expect(events).toContainEqual({
			type: 'live_block',
			index: 0,
			block: { type: 'compaction', text: '' }
		});
		expect(events).toContainEqual({
			type: 'message',
			message: expect.objectContaining({
				kind: 'compaction',
				summary: 'Anna asked for the files; ls found a.txt.'
			}),
			replacesLive: true
		});
	});

	it('stops, saying why, when the model writes no summary', async () => {
		const chat = haikuChatAsking(modelReply([listFiles], 'tool_use'));
		await run(chat.id);

		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(2);
		expect(getSnapshot(chat.id)).toMatchObject({
			running: false,
			error: expect.stringContaining("Haiku didn't write the summary this chat needs"),
			live: []
		});
		expect(committedRows(chat.id).at(-1)?.kind).toBe('tool_results');
	});
});

describe('summarizing on request', () => {
	const summarized = (text: string) =>
		modelReply([{ type: 'text', text: `<summary>${text}</summary>` }], 'end_turn');

	/** Resolves when the conversation's agent next stops working. */
	function idle(conversationId: string): Promise<void> {
		return new Promise((resolve) => {
			const off = onRunningChange((id, running) => {
				if (id !== conversationId || running) return;
				off();
				resolve();
			});
		});
	}

	it('has the model summarize the chat, without starting a turn', async () => {
		const chat = chatAsking(modelReply([{ type: 'text', text: 'Two files.' }], 'end_turn'));
		await run(chat.id);
		vi.mocked(streamTurn).mockResolvedValueOnce(summarized('Anna asked for the files: two.'));

		const done = idle(chat.id);
		compactConversation(chat.id, 'Anna');
		expect(getSnapshot(chat.id).running).toBe(true);
		await done;

		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(2);
		const ask = vi.mocked(streamTurn).mock.calls[1][0];
		expect(ask.compactAt).toBeUndefined();
		expect(toAnthropicMessages(ask.messages).at(-1)).toEqual({
			role: 'user',
			content: [{ type: 'text', text: summaryRequest('asked') }]
		});
		expect(committedRows(chat.id).map((row) => row.kind)).toEqual([
			'human',
			'assistant',
			'compaction'
		]);
		expect(getSnapshot(chat.id)).toMatchObject({ running: false, error: null, live: [] });
		// The reply before it was answered: Continue has nothing to answer.
		await run(chat.id);
		expect(vi.mocked(streamTurn)).toHaveBeenCalledTimes(2);
	});

	it('answers a message sent meanwhile from the summary, once it is written', async () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		insertQueued({ conversationId: chat.id, senderId: user.id, senderName: 'Anna', text: 'Hi' });
		vi.mocked(streamTurn).mockResolvedValueOnce(
			modelReply([{ type: 'text', text: 'Hello.' }], 'end_turn')
		);
		await run(chat.id);
		let write!: (reply: Anthropic.Message) => void;
		vi.mocked(streamTurn).mockReturnValueOnce(new Promise((resolve) => (write = resolve)));
		vi.mocked(streamTurn).mockResolvedValueOnce(
			modelReply([{ type: 'text', text: 'Fine, thanks.' }], 'end_turn')
		);

		compactConversation(chat.id, 'Anna');
		const answered = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'How are you?');
		write(summarized('Anna said hi.'));
		await answered;

		const next = vi.mocked(streamTurn).mock.calls[2][0];
		expect(toAnthropicMessages(next.messages)).toEqual([
			{ role: 'user', content: [{ type: 'text', text: expect.stringContaining('Anna said hi.') }] },
			{ role: 'user', content: [{ type: 'text', text: 'Anna: How are you?' }] }
		]);
	});

	it('refuses while the agent works, with nothing new, and on the Claude plan', async () => {
		const chat = chatAsking(modelReply([{ type: 'text', text: 'Two files.' }], 'end_turn'));
		expect(() => compactConversation(chat.id, 'Anna')).toThrow('nothing new to summarize');
		await run(chat.id);
		vi.mocked(streamTurn).mockResolvedValueOnce(summarized('Anna asked for the files.'));
		const done = idle(chat.id);
		compactConversation(chat.id, 'Anna');
		expect(() => compactConversation(chat.id, 'Anna')).toThrow(CompactionError);
		await done;
		expect(() => compactConversation(chat.id, 'Anna')).toThrow('nothing new to summarize');

		const plan = makePreset('Claude plan', 'opus', 'claude-plan');
		changeModel(chat.id, plan.id);
		expect(() => compactConversation(chat.id, 'Anna')).toThrow('Claude Code');
	});
});

describe('typing', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('tells everyone watching who is writing, until they stop or go quiet', () => {
		vi.useFakeTimers();
		const { user: anna, profile } = makeFamily();
		const max = makeUser('Max');
		const chat = createConversation({ profile, presetId: makePreset().id, userId: anna.id });
		const events: LiveEvent[] = [];
		const off = subscribe(chat.id, (event) => events.push(event));

		setTyping(chat.id, anna, true);
		setTyping(chat.id, max, true);
		vi.advanceTimersByTime(TYPING_TTL_MS - 1_000);
		// Anna is still at it, which is nothing new, and she counts as typing for longer.
		setTyping(chat.id, anna, true);
		expect(getSnapshot(chat.id).typing).toEqual([anna, max]);
		// Max's page stopped saying he's typing.
		vi.advanceTimersByTime(1_000);
		setTyping(chat.id, anna, false);
		setTyping(chat.id, anna, false);
		off();

		expect(events).toEqual([
			{ type: 'typing', typing: [anna] },
			{ type: 'typing', typing: [anna, max] },
			{ type: 'typing', typing: [anna] },
			{ type: 'typing', typing: [] }
		]);
		expect(getSnapshot(chat.id).typing).toEqual([]);
	});

	it('stops once their message shows', async () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		vi.mocked(streamTurn).mockResolvedValue(
			modelReply([{ type: 'text', text: 'Hi!' }], 'end_turn')
		);
		setTyping(chat.id, user, true);
		const events: LiveEvent[] = [];
		const off = subscribe(chat.id, (event) => events.push(event));
		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Hello');
		await ended;
		off();

		const types = events.map((event) => event.type);
		expect(types.indexOf('queued')).toBeLessThan(types.indexOf('typing'));
		expect(events.filter((event) => event.type === 'typing')).toEqual([
			{ type: 'typing', typing: [] }
		]);
		expect(getSnapshot(chat.id).typing).toEqual([]);
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

describe('auto mode', () => {
	beforeEach(() => {
		saveCommandMode('auto');
		vi.spyOn(console, 'log').mockImplementation(() => {});
	});

	const tidy = (id: string, command: string) => ({
		type: 'tool_use',
		id,
		name: 'run_command',
		input: { summary: 'Tidying up', command }
	});
	const said = (text: string) => modelReply([{ type: 'text', text }], 'end_turn');

	/** A named chat (naming one would ask the model too) where Anna asked to tidy up. */
	function tidyingChat(...replies: Anthropic.Message[]) {
		const { user, profile } = makeFamily();
		const chat = createConversation({
			profile,
			presetId: makePreset().id,
			userId: user.id,
			title: 'Downloads'
		});
		insertQueued({
			conversationId: chat.id,
			senderId: user.id,
			senderName: 'Anna',
			text: 'Tidy up my Downloads'
		});
		for (const reply of replies) vi.mocked(streamTurn).mockResolvedValueOnce(reply);
		return { chat, user };
	}

	/** What the check's model answers, in order. */
	function checkSays(...texts: string[]) {
		for (const text of texts) vi.mocked(createMessage).mockResolvedValueOnce(said(text));
	}

	it("answers a blocked command with the check's reason, without running it", async () => {
		const { chat } = tidyingChat(
			modelReply(
				[
					{ type: 'text', text: 'Anna surely wants it all gone.' },
					tidy('t1', 'rm -rf ~/Downloads')
				],
				'tool_use'
			),
			said('I need your OK to empty Downloads.')
		);
		checkSays('BLOCK', "Verdict: BLOCK\nReason: Anna didn't ask to delete everything.");

		await run(chat.id);

		expect(runCommand).not.toHaveBeenCalled();
		const [[blocked]] = results(chat.id);
		expect(blocked).toMatchObject({ callId: 't1', isError: true });
		expect(blocked.content).toMatch(
			/^Blocked by auto mode: Anna didn't ask to delete everything\.\n\nThe command didn't run/
		);
		expect(getSnapshot(chat.id).error).toBeNull();
		// The check read what Anna asked for and the command, not what the agent said.
		const { system, input } = vi.mocked(createMessage).mock.calls[0][0];
		expect(system).toMatch(/safety check/);
		expect(input).toContain('Message from Anna: Tidy up my Downloads');
		expect(input).toContain('rm -rf ~/Downloads');
		expect(input).not.toContain('surely wants');
	});

	it('runs what the check allows, and what only looks without asking', async () => {
		const { chat } = tidyingChat(
			modelReply(
				[tidy('t1', 'ls ~/Downloads'), tidy('t2', 'rm ~/Downloads/setup.dmg')],
				'tool_use'
			),
			said('Removed the installer.')
		);
		checkSays('ALLOW');
		vi.mocked(runCommand).mockResolvedValue({
			content: '[exit code 0]',
			isError: false,
			exitCode: 0
		});

		await run(chat.id);

		expect(runCommand).toHaveBeenCalledTimes(2);
		expect(createMessage).toHaveBeenCalledTimes(1);
	});

	it('tells the agent to stop after three blocks in a row, ends the loop if it goes on, and starts again when Anna answers', async () => {
		const { chat, user } = tidyingChat(
			modelReply([tidy('t1', 'rm -rf ~/Downloads')], 'tool_use'),
			modelReply([tidy('t2', 'find ~/Downloads -delete')], 'tool_use'),
			modelReply([tidy('t3', 'mv ~/Downloads ~/.Trash/')], 'tool_use'),
			modelReply([tidy('t4', 'rmdir ~/Downloads')], 'tool_use')
		);
		for (let i = 0; i < 3; i++) checkSays('BLOCK', 'Verdict: BLOCK\nReason: Not asked for.');

		await run(chat.id);

		const answered = results(chat.id).map(([result]) => result.content as string);
		expect(answered).toHaveLength(4);
		expect(answered[1]).not.toMatch(/Run no more commands/);
		expect(answered[2]).toMatch(/That's 3 commands blocked in a row\. Run no more commands now/);
		// Refused without asking the check again, and the loop ended there.
		expect(answered[3]).toMatch(/^Blocked by auto mode: no more commands run in this turn/);
		expect(createMessage).toHaveBeenCalledTimes(6);
		expect(streamTurn).toHaveBeenCalledTimes(4);
		expect(getSnapshot(chat.id).error).toBe(SAFETY_STOP);

		// Anna says to go ahead: the count starts again, and the check sees her answer.
		vi.mocked(streamTurn).mockResolvedValueOnce(
			modelReply([tidy('t5', 'rm -rf ~/Downloads')], 'tool_use')
		);
		vi.mocked(streamTurn).mockResolvedValueOnce(said('Done.'));
		checkSays('ALLOW');
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: '[exit code 0]',
			isError: false,
			exitCode: 0
		});
		const ended = loopEnd(chat.id);
		await sendMessage(chat.id, user, 'Yes, delete all of it');
		await ended;

		expect(runCommand).toHaveBeenCalledTimes(1);
		expect(getSnapshot(chat.id).error).toBeNull();
		const { input } = vi.mocked(createMessage).mock.calls.at(-1)![0];
		expect(input).toContain('Message from Anna: Yes, delete all of it');
		expect(input).toContain('Command you blocked: rm -rf ~/Downloads');
	});

	it('answers the call when someone presses Stop during the check', async () => {
		const { chat } = tidyingChat(modelReply([tidy('t1', 'rm ~/Downloads/a.zip')], 'tool_use'));
		vi.mocked(createMessage).mockReturnValueOnce(new Promise(() => {}));

		const ended = run(chat.id);
		await vi.waitFor(() => expect(createMessage).toHaveBeenCalled());
		stop(chat.id, 'Anna');
		await ended;

		expect(runCommand).not.toHaveBeenCalled();
		expect(results(chat.id)).toEqual([
			[{ type: 'tool_result', callId: 't1', content: 'Not run. Stopped by Anna.', isError: true }]
		]);
	});

	it("goes by the chat's own mode, changed for everyone who has it open", async () => {
		const { chat } = tidyingChat(
			modelReply([tidy('t1', 'rm -rf ~/Downloads')], 'tool_use'),
			said('Emptied it.')
		);
		const events: LiveEvent[] = [];
		subscribe(chat.id, (event) => events.push(event));
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: '[exit code 0]',
			isError: false,
			exitCode: 0
		});

		expect(changeCommandMode(chat.id, 'unrestricted')).toEqual({
			mode: 'unrestricted',
			fallback: 'auto'
		});
		await run(chat.id);

		expect(runCommand).toHaveBeenCalledTimes(1);
		expect(createMessage).not.toHaveBeenCalled();
		expect(events).toContainEqual({
			type: 'commands',
			commands: { mode: 'unrestricted', fallback: 'auto' }
		});
		expect(getSnapshot(chat.id).commands).toEqual({ mode: 'unrestricted', fallback: 'auto' });
	});

	it('checks a chat set to auto when Models & keys says unrestricted', async () => {
		saveCommandMode('unrestricted');
		const { chat } = tidyingChat(
			modelReply([tidy('t1', 'rm -rf ~/Downloads')], 'tool_use'),
			said('I need your OK first.')
		);
		changeCommandMode(chat.id, 'auto');
		checkSays('BLOCK', "Verdict: BLOCK\nReason: Anna didn't ask to delete everything.");

		await run(chat.id);

		expect(runCommand).not.toHaveBeenCalled();
		expect(results(chat.id)[0][0].content).toMatch(/^Blocked by auto mode/);
	});

	it("starts subagents with their chat's mode, and changes theirs with it", () => {
		const { chat } = tidyingChat();
		changeCommandMode(chat.id, 'unrestricted');
		const { conversation: sub } = runSubagent({
			parentId: chat.id,
			name: 'sorter',
			prompt: 'Sort.'
		});
		const modeOf = (id: string) =>
			getDb().select().from(conversation).where(eq(conversation.id, id)).get()?.commandMode;

		expect(modeOf(sub.id)).toBe('unrestricted');
		changeCommandMode(chat.id, null);
		expect(modeOf(sub.id)).toBeNull();
	});

	it('checks nothing when commands are unrestricted', async () => {
		saveCommandMode('unrestricted');
		const { chat } = tidyingChat(
			modelReply([tidy('t1', 'rm -rf ~/Downloads')], 'tool_use'),
			said('Emptied it.')
		);
		vi.mocked(runCommand).mockResolvedValueOnce({
			content: '[exit code 0]',
			isError: false,
			exitCode: 0
		});

		await run(chat.id);

		expect(runCommand).toHaveBeenCalledTimes(1);
		expect(createMessage).not.toHaveBeenCalled();
	});
});
