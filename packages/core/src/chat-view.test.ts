import { afterEach, describe, expect, it, vi } from 'vitest';
import { ChatView, TranscriptDiff, streamTranscript, type TranscriptUpdate } from './chat-view.ts';
import type { DisplayMessage } from './conversations.ts';
import type { LiveEvent, Snapshot } from './runner.ts';
import type { Reply, TextPart } from './transcript.ts';

const human = (id: number, text: string): DisplayMessage => ({
	id,
	kind: 'human',
	senderId: 'anna',
	senderName: 'Anna',
	text,
	attachments: [],
	queued: false,
	createdAt: id
});

const said = (id: number, text: string): DisplayMessage => ({
	id,
	kind: 'assistant',
	blocks: [{ type: 'text', text }],
	media: {},
	stopReason: 'end_turn',
	usage: null,
	provider: null,
	model: null,
	createdAt: id
});

/** A chat's snapshot, with only what the transcript is built from filled in. */
function snapshot(over: Partial<Snapshot> = {}): Snapshot {
	return {
		title: 'Rain',
		model: null,
		commands: {} as Snapshot['commands'],
		toolChanges: null,
		running: false,
		error: null,
		messages: [],
		queued: [],
		live: [],
		toolOutput: null,
		background: [],
		memory: [],
		typing: [],
		...over
	};
}

/** The text of the reply at `index`, as shown. */
function replyText(view: ChatView, index: number): string {
	const reply = view.transcript().entries[index] as Reply;
	return reply.parts.map((p) => (p as TextPart).text).join('');
}

afterEach(() => {
	vi.useRealTimers();
});

describe('ChatView', () => {
	it('builds the transcript from the snapshot and the events after it', () => {
		const view = new ChatView(snapshot({ messages: [human(1, 'Rain today?')] }));
		expect(view.apply({ type: 'status', running: true, error: null })).toBe(true);
		expect(
			view.apply({ type: 'live_block', index: 0, block: { type: 'text', text: 'Yes,' } })
		).toBe(true);
		expect(view.apply({ type: 'live_delta', index: 0, text: ' at 3pm.' })).toBe(true);
		const live = view.transcript().entries[1] as Reply;
		expect(live).toMatchObject({ type: 'reply', live: true });
		expect(replyText(view, 1)).toBe('Yes, at 3pm.');

		// Saved, the reply takes the live one's place, with the same key.
		view.apply({ type: 'message', message: said(2, 'Yes, at 3pm.'), replacesLive: true });
		view.apply({ type: 'status', running: false, error: null });
		const saved = view.transcript().entries[1] as Reply;
		expect(saved).toMatchObject({ key: live.key, live: false });
		expect(saved.parts).toEqual([expect.objectContaining({ text: 'Yes, at 3pm.' })]);
	});

	it("keeps the results of commands, and ignores events that aren't the transcript's", () => {
		const view = new ChatView(snapshot());
		const result = { id: 'call-1', output: 'ok', isError: false, pictures: [] };
		view.apply({
			type: 'message',
			message: { id: 1, kind: 'tool_results', results: [result], createdAt: 1 }
		});
		expect(view.transcript().results).toEqual({ 'call-1': result });
		expect(view.apply({ type: 'title', title: 'Umbrellas' })).toBe(false);
		expect(view.apply({ type: 'status', running: false, error: null })).toBe(false);
	});

	it("doesn't change the blocks the runner holds, which it goes on writing", () => {
		const held = { type: 'text' as const, text: 'Yes' };
		const view = new ChatView(
			snapshot({ messages: [human(1, 'Rain?')], live: [held], running: true })
		);
		view.apply({ type: 'live_delta', index: 0, text: ', at 3pm.' });
		expect(held.text).toBe('Yes');

		const sent = { type: 'thinking' as const, text: '' };
		view.apply({ type: 'live_block', index: 1, block: sent });
		view.apply({ type: 'live_delta', index: 1, text: 'Checking' });
		expect(sent.text).toBe('');
	});
});

describe('TranscriptDiff', () => {
	it('sends everything at first, then only what changed', () => {
		const view = new ChatView(snapshot({ messages: [human(1, 'Rain?')], running: true }));
		const diff = new TranscriptDiff();
		const first = diff.next(view.transcript())!;
		expect(first.order).toHaveLength(2);
		expect(first.entries.map((e) => e.key)).toEqual(first.order);
		expect(diff.next(view.transcript())).toBeNull();

		view.apply({ type: 'live_block', index: 0, block: { type: 'text', text: 'Yes' } });
		const update = diff.next(view.transcript())!;
		expect(update.order).toEqual(first.order);
		expect(update.entries.map((e) => e.type)).toEqual(['reply']);
	});

	it('says when an entry is gone, and sends each result once', () => {
		const view = new ChatView(snapshot({ messages: [human(1, 'Rain?')], running: true }));
		const diff = new TranscriptDiff();
		diff.next(view.transcript());
		const result = { id: 'call-1', output: 'ok', isError: false, pictures: [] };
		view.apply({
			type: 'message',
			message: { id: 2, kind: 'tool_results', results: [result], createdAt: 2 }
		});
		view.apply({ type: 'status', running: false, error: null });
		const update = diff.next(view.transcript())!;
		// Nothing running and nothing written: the empty live reply is gone.
		expect(update.order).toEqual(['m1']);
		expect(update.results).toEqual({ 'call-1': result });
		expect(diff.next(view.transcript())).toBeNull();
	});
});

describe('streamTranscript', () => {
	function stream(initial: Snapshot) {
		const sent: { type: string; [key: string]: unknown }[] = [];
		let listener: (event: LiveEvent) => void = () => {};
		const stop = streamTranscript(
			initial,
			(l) => {
				listener = l;
				return () => (listener = () => {});
			},
			(data) => sent.push(data as { type: string })
		);
		return { sent, emit: (event: LiveEvent) => listener(event), stop };
	}

	it('starts with the snapshot, the transcript built in place of its rows', () => {
		const { sent } = stream(snapshot({ messages: [human(1, 'Rain?')] }));
		expect(sent).toHaveLength(1);
		expect(sent[0]).toMatchObject({ type: 'snapshot', snapshot: { title: 'Rain', results: {} } });
		const shown = sent[0].snapshot as Record<string, unknown>;
		expect(shown.entries).toEqual([expect.objectContaining({ key: 'm1', type: 'human' })]);
		expect(shown).not.toHaveProperty('messages');
		expect(shown).not.toHaveProperty('live');
	});

	it('sends a streaming reply at most every 100 ms, before any other event', () => {
		vi.useFakeTimers();
		const { sent, emit, stop } = stream(snapshot({ messages: [human(1, 'Rain?')], running: true }));
		emit({ type: 'live_block', index: 0, block: { type: 'text', text: 'Yes' } });
		emit({ type: 'live_delta', index: 0, text: ',' });
		emit({ type: 'live_delta', index: 0, text: ' at 3pm.' });
		expect(sent).toHaveLength(1);
		vi.advanceTimersByTime(100);
		expect(sent.map((s) => s.type)).toEqual(['snapshot', 'transcript']);
		const update = sent[1] as unknown as TranscriptUpdate;
		const reply = update.entries[0] as Reply;
		expect((reply.parts[0] as TextPart).text).toBe('Yes, at 3pm.');

		// What's waiting goes before the event after it, so the reply ends before the status says so.
		emit({ type: 'message', message: said(2, 'Yes, at 3pm.'), replacesLive: true });
		emit({ type: 'status', running: false, error: null });
		expect(sent.map((s) => s.type)).toEqual(['snapshot', 'transcript', 'transcript', 'status']);
		expect((sent[2] as unknown as TranscriptUpdate).entries[0]).toMatchObject({ live: false });

		emit({ type: 'title', title: 'Umbrellas' });
		expect(sent.at(-1)).toEqual({ type: 'title', title: 'Umbrellas' });

		// Stopped, nothing more goes out, a pending update included.
		emit({ type: 'live_block', index: 0, block: { type: 'text', text: 'More' } });
		stop();
		vi.advanceTimersByTime(1000);
		expect(sent).toHaveLength(5);
	});
});
