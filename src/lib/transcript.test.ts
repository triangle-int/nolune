import type { DisplayMemoryLook, DisplayMessage } from '@btw/core';
import { describe, expect, it } from 'vitest';
import { buildTranscript } from './transcript';

const human = (id: number, text: string): DisplayMessage => ({
	id,
	kind: 'human',
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
			undone: null
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
		// A look between two of btw's rows (it read the chat while a background command ran on).
		const entries = buildTranscript([human(1, 'Hi'), said(2, 'One'), said(4, 'Two')], [], false, [
			look(2)
		]);
		expect(entries.map((entry) => entry.type)).toEqual(['human', 'reply', 'memory', 'reply']);
		expect(new Set(entries.map((entry) => entry.key)).size).toBe(entries.length);
	});
});
