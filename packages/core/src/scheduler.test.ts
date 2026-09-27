import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { streamTurn } from './anthropic.ts';
import { getDb } from './db/index.ts';
import { notification } from './db/schema.ts';
import { runCommand, type RunCommandResult } from './run-command.ts';
import { onLoopEnd } from './runner.ts';
import { runTriggerNow, startScheduler } from './scheduler.ts';
import { createTrigger, getRun } from './triggers.ts';
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

/** Resolves when any conversation's loop next stops, with its id. */
function nextLoopEnd(): Promise<string> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			off();
			resolve(id);
		});
	});
}

describe('automation runs', () => {
	it('wait for their background commands and notify with the reply that comes after', async () => {
		startScheduler();
		const { profile } = makeFamily();
		makePreset();
		const t = createTrigger({
			profileId: profile.id,
			name: 'Photos',
			when: { kind: 'webhook' },
			what: { action: 'agent', prompt: 'Download the new photos.' }
		});
		vi.mocked(streamTurn)
			.mockResolvedValueOnce(
				modelReply(
					[
						{
							type: 'tool_use',
							id: 'bg',
							name: 'run_command',
							input: { summary: 'Downloading', command: 'fetch', run_in_background: true }
						}
					],
					'tool_use'
				)
			)
			.mockResolvedValueOnce(
				modelReply([{ type: 'text', text: 'Started the download.' }], 'end_turn')
			)
			.mockResolvedValueOnce(modelReply([{ type: 'text', text: '120 new photos.' }], 'end_turn'));
		let finish!: (result: RunCommandResult) => void;
		vi.mocked(runCommand).mockImplementationOnce((_input, options) => {
			options.onStart?.(4242);
			return new Promise((resolve) => (finish = resolve));
		});

		const first = nextLoopEnd();
		const run = runTriggerNow(t);
		await first;
		expect(getRun(run.id)?.status).toBe('running');
		expect(getDb().select().from(notification).all()).toEqual([]);

		const second = nextLoopEnd();
		finish({ content: 'saved\n[exit code 0]', isError: false, exitCode: 0 });
		await second;
		expect(getRun(run.id)?.status).toBe('notified');
		expect(getDb().select().from(notification).all()).toMatchObject([{ body: '120 new photos.' }]);
	});
});
