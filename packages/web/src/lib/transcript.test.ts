import type { DisplayMemoryLook, DisplayMessage } from '@nolune/core';
import { describe, expect, it } from 'vitest';
import type { Messages } from './i18n';
import {
	activeStepLabel,
	buildTranscript,
	resultStatus,
	type ActivityPart,
	type Reply
} from './transcript';

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

const look = (after: number): DisplayMemoryLook => ({
	after,
	createdAt: after,
	changes: [
		{
			id: after,
			op: 'add',
			note: 'pets.md',
			fact: 'The dog is called Rex',
			before: null,
			createdAt: after,
			undone: null,
			card: null
		}
	]
});

describe('what the note-taker saved', () => {
	it('goes after the last message it read, before what came next', () => {
		const messages = [
			human(1, 'We got a dog'),
			said(2, 'Congratulations!'),
			human(5, 'His name is Rex'),
			said(6, 'Rex it is.')
		];
		const entries = buildTranscript(messages, [], false, [look(6), look(2)]);
		expect(entries.map((entry) => entry.type)).toEqual([
			'human',
			'reply',
			'memory',
			'human',
			'reply',
			'memory'
		]);
		expect(new Set(entries.map((entry) => entry.key)).size).toBe(entries.length);
	});

	it("doesn't split a reply's key with the one after it", () => {
		// A look between two of nolune's rows (it read the chat while a background command ran on).
		const entries = buildTranscript([human(1, 'Hi'), said(2, 'One'), said(4, 'Two')], [], false, [
			look(2)
		]);
		expect(entries.map((entry) => entry.type)).toEqual(['human', 'reply', 'memory', 'reply']);
		expect(new Set(entries.map((entry) => entry.key)).size).toBe(entries.length);
	});
});

describe('summaries of the conversation', () => {
	const usage = (input: number, output: number) => ({ input, cacheRead: 0, cacheWrite: 0, output });
	const m = { steps: { summarizing: 'Summarizing' } } as unknown as Messages;

	it("show as a step of nolune's work, with what writing them took", () => {
		const messages: DisplayMessage[] = [
			human(1, 'Files?'),
			// The runner's, before the reply…
			{ id: 2, kind: 'compaction', summary: 'Anna asked.', usage: usage(10, 3), createdAt: 2 },
			// …or Claude's, starting it.
			{
				id: 3,
				kind: 'assistant',
				blocks: [
					{ type: 'compaction', summary: 'Anna asked again.' },
					{ type: 'text', text: 'Two.' }
				],
				media: {},
				stopReason: 'end_turn',
				usage: { ...usage(5, 1), compaction: usage(100, 20) },
				provider: null,
				model: null,
				createdAt: 3
			}
		];
		const [, reply] = buildTranscript(messages, [], false);
		expect(reply).toMatchObject({
			type: 'reply',
			parts: [
				{
					type: 'activity',
					steps: [
						{ type: 'compaction', summary: 'Anna asked.' },
						{ type: 'compaction', summary: 'Anna asked again.' }
					]
				},
				{ type: 'text', text: 'Two.' }
			],
			usage: usage(115, 24)
		});
	});

	it('say so while the model is writing one', () => {
		const entries = buildTranscript([human(1, 'Files?')], [{ type: 'compaction', text: '' }], true);
		const reply = entries[1] as Reply;
		const part = reply.parts[0] as ActivityPart;
		expect(part.steps).toEqual([{ type: 'compaction', summary: '' }]);
		expect(activeStepLabel(part, {}, false, m)).toBe('Summarizing');
	});
});

describe('resultStatus', () => {
	const result = (output: string, isError: boolean) => ({ output, isError, pictures: [] });

	it('tells a command auto mode blocked from one that failed or was stopped', () => {
		expect(resultStatus(result('a.txt\n[exit code 0]', false))).toBe('done');
		expect(resultStatus(result('No such file\n[exit code 1]', true))).toBe('failed');
		expect(resultStatus(result('partial\n[Stopped by Anna.]', true))).toBe('stopped');
		expect(
			resultStatus(
				result(
					"Blocked by auto mode: Anna didn't ask to delete it.\n\nThe command didn't run.",
					true
				)
			)
		).toBe('blocked');
	});
});
