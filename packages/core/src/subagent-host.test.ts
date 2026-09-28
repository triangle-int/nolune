import { readFileSync } from 'node:fs';
import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { streamTurn } from './anthropic.ts';
import { createConversation } from './conversations.ts';
import { TOOLS } from './run-command.ts';
import { onLoopEnd } from './runner.ts';
import { finishSubagent, processSubagents, stopConversation } from './subagent-host.ts';
import {
	findSubagent,
	runSubagent,
	setSubagentStatus,
	steerSubagent,
	subagentLogPath,
	subagentResult,
	type Subagent
} from './subagents.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	streamTurn: vi.fn()
}));

onLoopEnd(finishSubagent);

afterEach(() => {
	vi.resetAllMocks();
});

function modelReply(content: unknown[]): Anthropic.Message {
	return {
		id: 'msg',
		type: 'message',
		role: 'assistant',
		model: 'claude-sonnet-5',
		content,
		stop_reason: 'end_turn',
		stop_sequence: null,
		usage: { input_tokens: 10, output_tokens: 5 }
	} as unknown as Anthropic.Message;
}

function startSubagent(prompt = 'Find flights to Lisbon.'): Subagent {
	const { user, profile } = makeFamily();
	const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
	return runSubagent({ parentId: chat.id, prompt }).subagent;
}

/** Resolves when the conversation's loop has stopped (after the host has recorded how). */
function loopEnd(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
	});
}

describe('the subagent host', () => {
	it('runs a pending subagent with a 5-minute cache and records its last message', async () => {
		const s = startSubagent();
		vi.mocked(streamTurn).mockResolvedValueOnce(
			modelReply([
				{ type: 'thinking', thinking: 'Which airlines fly there?', signature: 'sig' },
				{ type: 'text', text: 'TAP at 7:05, 89 € each.' }
			])
		);

		const ended = loopEnd(s.conversationId);
		processSubagents();
		expect(findSubagent(s.parentId, s.name)?.status).toBe('running');
		await ended;

		const request = vi.mocked(streamTurn).mock.calls[0][0];
		expect(request.cacheTtl).toBe('5m');
		expect(request.tools).toEqual(TOOLS);
		expect(request.messages).toHaveLength(1);
		expect(JSON.stringify(request.messages[0])).toContain('Find flights to Lisbon.');

		const done = findSubagent(s.parentId, s.name)!;
		expect(done.status).toBe('done');
		expect(subagentResult(done)).toEqual({ ok: true, text: 'TAP at 7:05, 89 € each.' });

		const log = readFileSync(subagentLogPath(s)!, 'utf8');
		expect(log).toContain('Find flights to Lisbon.');
		expect(log).toContain('TAP at 7:05, 89 € each.');
		expect(log).toContain('done.');
		expect(log).not.toContain('Which airlines');
	});

	it('answers a steer that arrives while it works before it counts as done', async () => {
		const s = startSubagent();
		let answer!: (reply: Anthropic.Message) => void;
		vi.mocked(streamTurn)
			.mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)))
			.mockResolvedValueOnce(modelReply([{ type: 'text', text: 'Mornings: TAP at 7:05.' }]))
			.mockResolvedValueOnce(modelReply([{ type: 'text', text: 'unused' }]));

		const ended = loopEnd(s.conversationId);
		processSubagents();
		await vi.waitFor(() => expect(streamTurn).toHaveBeenCalledTimes(1));
		steerSubagent({ parentId: s.parentId, name: s.name, text: 'Mornings only.' });
		answer(modelReply([{ type: 'text', text: 'TAP at 7:05 or 18:40.' }]));
		await ended;

		const steered = vi.mocked(streamTurn).mock.calls[1][0].messages;
		expect(JSON.stringify(steered.at(-1))).toContain('Mornings only.');
		// The steer set it pending while it ran; the next tick settles it.
		expect(findSubagent(s.parentId, s.name)?.status).toBe('pending');
		const settled = loopEnd(s.conversationId);
		processSubagents();
		await settled;
		const done = findSubagent(s.parentId, s.name)!;
		expect(done.status).toBe('done');
		expect(subagentResult(done)?.text).toBe('Mornings: TAP at 7:05.');
		expect(streamTurn).toHaveBeenCalledTimes(2);
	});

	it('marks a subagent whose model call failed as failed', async () => {
		const s = startSubagent();
		vi.mocked(streamTurn).mockRejectedValueOnce(new Error('overloaded'));
		vi.spyOn(console, 'error').mockImplementation(() => {});

		const ended = loopEnd(s.conversationId);
		processSubagents();
		await ended;

		expect(findSubagent(s.parentId, s.name)).toMatchObject({
			status: 'failed',
			error: 'overloaded'
		});
	});

	it('stops the subagents of a chat when someone presses Stop there', async () => {
		const s = startSubagent();
		vi.mocked(streamTurn).mockImplementationOnce(
			({ signal }) =>
				new Promise((_, reject) => {
					signal.addEventListener('abort', () => reject(new Error('aborted')));
				})
		);

		const ended = loopEnd(s.conversationId);
		processSubagents();
		await vi.waitFor(() => expect(streamTurn).toHaveBeenCalled());
		stopConversation(s.parentId, 'Anna');
		await ended;

		expect(findSubagent(s.parentId, s.name)).toMatchObject({
			status: 'stopped',
			error: 'Stopped by Anna.'
		});
	});

	it('stops a subagent from its own chat while it only waits, with no loop left to end', () => {
		const s = startSubagent();
		setSubagentStatus(s.id, 'running');
		stopConversation(s.conversationId, 'Anna');
		expect(findSubagent(s.parentId, s.name)).toMatchObject({
			status: 'stopped',
			error: 'Stopped by Anna.'
		});
	});

	it('stops a pending subagent that `nolune agent stop` asked to stop without starting it', () => {
		const s = startSubagent();
		stopConversation(s.parentId, 'Anna');
		expect(findSubagent(s.parentId, s.name)?.status).toBe('stopped');
		expect(streamTurn).not.toHaveBeenCalled();
	});
});
