import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
	MemoryError,
	addMemoryFact,
	listMemoryFiles,
	mergeMemoryNotes,
	readMemoryNote,
	renameMemoryNote,
	writeMemoryNote
} from './memory.ts';
import { aliasesOf, categoryOf, nameKey, titleIn, whoIn } from './memory-categories.ts';
import { profileMemoryDir } from './paths.ts';
import { makeFamily } from './test/fixtures.ts';

describe('the categories', () => {
	it('are notes of their own, or a folder with a note per person or project', () => {
		expect(categoryOf('core.md')).toBe('core');
		expect(categoryOf('plans')).toBe('plans');
		expect(categoryOf('projects.md')).toBe('projects');
		expect(categoryOf('people/anna.md')).toBe('people');
		expect(categoryOf('projects/garden')).toBe('projects');
		expect(categoryOf('people.md')).toBeNull();
		expect(categoryOf('people/anna/school.md')).toBeNull();
		expect(categoryOf('home/wifi.md')).toBeNull();
		expect(categoryOf('family.md')).toBeNull();
	});

	it('take every new fact, and say where things go otherwise', () => {
		const { profile } = makeFamily();
		addMemoryFact(profile.slug, 'people/olga', "Anna's grandmother");
		expect(() => addMemoryFact(profile.slug, 'family', 'Anna is 7')).toThrow(
			/"family" isn't one of memory's categories: core, home, health, plans, routines, pets, places, projects, other, or a note of its own in people\/ or projects\//
		);
		expect(() => writeMemoryNote(profile.slug, 'kids', '- Mia is 7')).toThrow(MemoryError);
		expect(() => renameMemoryNote(profile.slug, 'people/olga', 'grandma')).toThrow(
			"isn't one of memory's categories"
		);
		expect(listMemoryFiles(profile.slug).map((f) => f.path)).toEqual(['people/olga.md']);
	});

	it('leave a note from before them readable and editable until it is moved into one', () => {
		const { profile } = makeFamily();
		writeMemoryNote(profile.slug, 'home', '- Wifi: mango42\n');
		writeFileSync(join(profileMemoryDir(profile.slug), 'family.md'), '# Family\n\n- Mia is 7\n');
		expect(() => addMemoryFact(profile.slug, 'family', 'Leo is 5')).toThrow(
			'To keep adding to it, move it into one first: `btw memory mv family <category>`.'
		);
		writeMemoryNote(profile.slug, 'family', '# Family\n\n- Mia is 8\n');
		expect(readMemoryNote(profile.slug, 'family').text).toBe('# Family\n\n- Mia is 8\n');
		renameMemoryNote(profile.slug, 'family', 'people/mia');
		addMemoryFact(profile.slug, 'people/mia', 'Plays piano');
		expect(readMemoryNote(profile.slug, 'people/mia').text).toBe(
			'# Mia\n\n- Mia is 8\n- Plays piano\n'
		);
		// Within people/, the title stays: it's their name, whatever the note is called.
		renameMemoryNote(profile.slug, 'people/mia', 'people/mia-k');
		expect(readMemoryNote(profile.slug, 'people/mia-k').text).toContain('# Mia\n');
	});

	it("title a person's new note with their whole name", () => {
		const { profile } = makeFamily();
		addMemoryFact(profile.slug, 'people/anna-maria', 'Likes tea');
		expect(readMemoryNote(profile.slug, 'people/anna-maria').text).toBe(
			'# Anna Maria\n\n- Likes tea\n'
		);
	});
});

describe("a person's note", () => {
	const note =
		"# Olga\n\n- Who: Anna's grandmother\n- Also called: grandma, **Granny Olga**; бабушка\n- Зовут: Оля\n\n## Contacts\n\n- Phone: 555-0101\n";

	it('says who they are and what the family calls them, in any of the languages', () => {
		expect(titleIn(note)).toBe('Olga');
		expect(whoIn(note)).toBe("Anna's grandmother");
		expect(aliasesOf(note)).toEqual(['grandma', 'Granny Olga', 'бабушка', 'Оля']);
		expect(aliasesOf('# Leo\n\n- Likes trains\n')).toEqual([]);
		expect(whoIn('- Кто: бабушка Анны')).toBe('бабушка Анны');
	});

	it('is matched by name in any alphabet, with or without accents', () => {
		expect(nameKey('Ольга')).toBe(nameKey('Olga'));
		expect(nameKey('José María')).toBe('josemaria');
		expect(nameKey('Anna-Lena')).toBe('annalena');
	});
});

describe('merging notes', () => {
	it('puts each part under its heading, leaves out what the other says already, and keeps the dates', () => {
		const { profile } = makeFamily();
		writeMemoryNote(
			profile.slug,
			'people/olga',
			"# Olga\n\n- Who: Anna's grandmother\n- Also called: Granny Olga\n\n## Contacts\n\n- Phone: 555-0101\n"
		);
		writeMemoryNote(
			profile.slug,
			'people/grandma',
			'# Grandma\n\n- Also called: бабушка\n- Loves roses\n  and tulips\n\n## Contacts\n\n- Phone: 555-0101\n- Lives in Tver\n\n## Recipes\n\n| Dish | When |\n| --- | --- |\n| Pirozhki | Sundays |\n'
		);
		const roses = listMemoryFiles(profile.slug).find((f) => f.path === 'people/grandma.md')!
			.facts[1].learnedAt;

		expect(mergeMemoryNotes(profile.slug, 'people/grandma', 'people/olga')).toEqual({
			from: 'people/grandma.md',
			into: 'people/olga.md',
			added: 3,
			merged: true
		});
		expect(readMemoryNote(profile.slug, 'people/olga').text).toBe(
			"# Olga\n\n- Who: Anna's grandmother\n- Also called: Granny Olga, Grandma, бабушка\n- Loves roses\n  and tulips\n\n## Contacts\n\n- Phone: 555-0101\n- Lives in Tver\n\n## Recipes\n\n| Dish | When |\n| --- | --- |\n| Pirozhki | Sundays |\n"
		);
		const files = listMemoryFiles(profile.slug);
		expect(files.map((f) => f.path)).toEqual(['people/olga.md']);
		expect(files[0].facts.find((f) => f.text === 'Loves roses and tulips')?.learnedAt).toBe(roses);
	});

	it('is a move when the other note is not there yet, and never of or into core', () => {
		const { profile } = makeFamily();
		mkdirSync(profileMemoryDir(profile.slug), { recursive: true });
		writeFileSync(join(profileMemoryDir(profile.slug), 'kids.md'), '# Kids\n\n- Mia is 7\n');
		expect(mergeMemoryNotes(profile.slug, 'kids', 'people/mia')).toEqual({
			from: 'kids.md',
			into: 'people/mia.md',
			added: 1,
			merged: false
		});
		addMemoryFact(profile.slug, 'core', 'We speak Russian');
		expect(() => mergeMemoryNotes(profile.slug, 'people/mia', 'core')).toThrow('core is pinned');
		expect(() => mergeMemoryNotes(profile.slug, 'people/mia', 'people/mia.md')).toThrow(
			'the same note'
		);
	});
});
