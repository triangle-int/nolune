import { describe, expect, it } from 'vitest';
import {
	emptyFactIndex,
	forgetFacts,
	moveFacts,
	noteFacts,
	parseFactIndex,
	parseFacts,
	readFacts,
	serializeFactIndex
} from './memory-facts.ts';

describe('parseFacts', () => {
	it('reads list items, paragraphs, table rows and code blocks as plain text', () => {
		const note = [
			'# People',
			'',
			'- Anna likes **tea**',
			'- [x] Max is 7',
			'',
			'The car is in [the garage](garage.md).',
			'',
			'| Name | Age |',
			'| --- | --- |',
			'| Anna | 40 |',
			'',
			'```',
			'wifi: hunter2',
			'```',
			'---',
			'> quoted _fact_'
		].join('\n');
		expect(parseFacts(note)).toEqual([
			'Anna likes tea',
			'Max is 7',
			'The car is in the garage.',
			'Anna · 40',
			'wifi: hunter2',
			'quoted fact'
		]);
	});
});

describe('readFacts', () => {
	it('says where each fact starts and which heading it is under, but not the title', () => {
		const note = [
			'# Anna',
			'- Likes tea',
			'',
			'## Health ##',
			'- Allergic to',
			'  peanuts',
			'| Doctor | Dr. Keller |',
			'### **School**',
			'Riverside'
		].join('\n');
		expect(readFacts(note)).toEqual([
			{ text: 'Likes tea', line: 2, heading: null },
			{ text: 'Allergic to peanuts', line: 5, heading: 'Health' },
			{ text: 'Doctor · Dr. Keller', line: 7, heading: 'Health' },
			{ text: 'Riverside', line: 9, heading: 'School' }
		]);
	});
});

describe('fact index', () => {
	it('keeps the first-seen date of facts that stay, ignoring case', () => {
		const index = emptyFactIndex();
		expect(noteFacts(index, 'a.md', '- one\n- two', 100)).toBe(true);
		expect(noteFacts(index, 'a.md', '- one\n- two', 200)).toBe(false);
		expect(noteFacts(index, 'a.md', '- One\n- three', 300)).toBe(true);
		expect(index.files.get('a.md')).toEqual(
			new Map([
				['one', 100],
				['three', 300]
			])
		);
		expect(index.removed).toEqual(new Map([['two', 100]]));
	});

	it('keeps the date of a fact that moves to another file', () => {
		const index = emptyFactIndex();
		noteFacts(index, 'a.md', '- two', 100);
		noteFacts(index, 'a.md', '', 200);
		noteFacts(index, 'b.md', '- two', 300);
		expect(index.files.get('b.md')).toEqual(new Map([['two', 100]]));
		expect(index.removed.size).toBe(0);
	});

	it('follows renamed and deleted folders', () => {
		const index = emptyFactIndex();
		noteFacts(index, 'family/anna.md', '- likes tea', 100);
		moveFacts(index, 'family', 'people');
		expect([...index.files.keys()]).toEqual(['people/anna.md']);
		forgetFacts(index, 'people');
		expect(index.files.size).toBe(0);
		expect(index.removed).toEqual(new Map([['likes tea', 100]]));
	});

	it('survives a round trip through JSON', () => {
		const index = emptyFactIndex();
		noteFacts(index, 'a.md', '- one\n- two', 100);
		noteFacts(index, 'a.md', '- one', 200);
		expect(parseFactIndex(serializeFactIndex(index))).toEqual(index);
	});

	it('rejects what it did not write', () => {
		expect(parseFactIndex('not json')).toBeNull();
		expect(parseFactIndex('{"version":2,"files":[],"removed":[]}')).toBeNull();
		expect(parseFactIndex('{"version":1,"files":[["a.md",[["x","y"]]]],"removed":[]}')).toBeNull();
	});
});
