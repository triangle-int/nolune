import type { ActivityPush } from '@nolune/relay/protocol';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DisplayMessage } from './conversations.ts';
import {
	activityState,
	followLiveActivity,
	followedForActivities,
	forgetLiveActivity
} from './live-activities.ts';
import type { LiveEvent, Snapshot } from './runner.ts';
import { buildTranscript } from './transcript.ts';

const TOKEN = 'a1'.repeat(32);

const asked = (id: number, text: string): DisplayMessage => ({
	id,
	kind: 'human',
	senderId: 'anna',
	senderName: 'Anna',
	text,
	attachments: [],
	queued: false,
	createdAt: id
});

const ran = (id: number, call: string, summary: string): DisplayMessage => ({
	id,
	kind: 'assistant',
	blocks: [{ type: 'tool', id: call, command: 'curl wttr.in', summary, icon: 'cloud-rain' }],
	media: {},
	stopReason: 'tool_use',
	usage: null,
	provider: null,
	model: null,
	createdAt: id
});

const said = (id: number, text: string): DisplayMessage => ({
	...(ran(id, '', '') as Extract<DisplayMessage, { kind: 'assistant' }>),
	blocks: [{ type: 'text', text }],
	stopReason: 'end_turn'
});

function snapshot(over: Partial<Snapshot> = {}): Snapshot {
	return {
		title: 'Rain',
		model: null,
		commands: {} as Snapshot['commands'],
		toolChanges: null,
		running: true,
		error: null,
		messages: [asked(1, 'Will it rain?')],
		queued: [],
		live: [],
		toolOutput: null,
		background: [],
		memory: [],
		typing: [],
		...over
	};
}

/** A chat followed for an activity, with its events in the test's hands and what went out. */
function follow(over: Partial<Snapshot> = {}) {
	const sent: ActivityPush[] = [];
	let listener: (event: LiveEvent) => void = () => {};
	let now = 1_000_000;
	followLiveActivity(
		'chat',
		{ token: TOKEN, sandbox: true },
		{
			snapshot: snapshot(over),
			subscribe: (l) => {
				listener = l;
				return () => (listener = () => {});
			},
			send: async (change) => {
				sent.push(change);
				return [];
			},
			now: () => now
		}
	);
	return {
		sent,
		emit: (event: LiveEvent) => listener(event),
		later: (ms: number) => {
			now += ms;
			vi.advanceTimersByTime(ms);
		}
	};
}

afterEach(() => {
	forgetLiveActivity('chat', TOKEN);
	vi.useRealTimers();
});

describe('what a Live Activity shows', () => {
	it('is the command nolune runs, in its own words, then the reply', () => {
		const working = buildTranscript(
			[asked(1, 'Rain?'), ran(2, 't1', 'Checking the forecast')],
			[],
			true
		);
		expect(activityState(working, {}, 'Rain', true)).toEqual({
			title: 'Rain',
			step: 'Checking the forecast',
			running: true
		});
		// Once the command is done, nolune thinks again: nothing to name.
		const results = { t1: { output: 'rain', isError: false, pictures: [] } };
		expect(activityState(working, results, 'Rain', true).step).toBe('');
		const done = buildTranscript(
			[
				asked(1, 'Rain?'),
				ran(2, 't1', 'Checking'),
				said(3, 'Yes, **at 3pm**. Take an [umbrella](https://x.y).')
			],
			[],
			false
		);
		expect(activityState(done, results, 'Rain', false)).toEqual({
			title: 'Rain',
			step: 'Yes, at 3pm. Take an umbrella.',
			running: false
		});
	});
});

describe('following a chat for a Live Activity', () => {
	it('sends what nolune does at most every 10 seconds, then the end, and stops', async () => {
		vi.useFakeTimers();
		const { sent, emit, later } = follow();
		await vi.runOnlyPendingTimersAsync();
		expect(sent).toHaveLength(1);
		expect(sent[0]).toMatchObject({
			activities: [{ token: TOKEN, sandbox: true }],
			event: 'update',
			state: { title: 'Rain', step: '', running: true }
		});

		emit({ type: 'message', message: ran(2, 't1', 'Checking the forecast') });
		emit({ type: 'title', title: 'Umbrellas' });
		expect(sent).toHaveLength(1);
		later(10_000);
		await Promise.resolve();
		expect(sent).toHaveLength(2);
		expect(sent[1].state).toEqual({
			title: 'Umbrellas',
			step: 'Checking the forecast',
			running: true
		});

		emit({ type: 'message', message: said(3, 'Yes, at 3pm.') });
		emit({ type: 'status', running: false, error: null });
		await Promise.resolve();
		expect(sent.at(-1)).toMatchObject({
			event: 'end',
			state: { title: 'Umbrellas', step: 'Yes, at 3pm.', running: false }
		});
		expect(sent.at(-1)!.dismissAt).toBeGreaterThan(1_000);
		expect(followedForActivities()).toEqual([]);
	});

	it("ends a reply that doesn't start within a minute", async () => {
		vi.useFakeTimers();
		const { sent, later } = follow({ running: false });
		later(60_000);
		await Promise.resolve();
		expect(sent.at(-1)).toMatchObject({ event: 'end' });
		expect(followedForActivities()).toEqual([]);
	});
});
