import { statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
	MAX_PINNED_CHARS,
	MemoryConflictError,
	addMemoryFact,
	readMemoryNote,
	readPinnedNote,
	renameMemoryNote,
	writeMemoryFile,
	writeMemoryNote
} from './memory.ts';
import { profileMemoryDir } from './paths.ts';
import { buildSystemPrompt } from './prompt.ts';
import { makeFamily } from './test/fixtures.ts';

describe('the core note', () => {
	it('is copied whole into the prompt, and left out of the list of notes', () => {
		const { profile } = makeFamily();
		addMemoryFact(profile.slug, 'core', 'Mia is allergic to nuts');
		addMemoryFact(profile.slug, 'food', 'Pizza on Fridays');
		const prompt = buildSystemPrompt(profile);
		expect(prompt).toContain('<note name="core">\n# Core\n\n- Mia is allergic to nuts\n</note>');
		expect(prompt).toContain('Other notes when this conversation started: food.');
		expect(prompt).not.toContain('Pizza on Fridays');
	});

	it('says so while it is empty', () => {
		const { profile } = makeFamily();
		const prompt = buildSystemPrompt(profile);
		expect(prompt).toContain('It is empty so far.');
		expect(prompt).toContain('There are no other notes yet.');
		expect(prompt).not.toContain('<note name="core">');
	});

	it('stays small, unlike other notes', () => {
		const { profile } = makeFamily();
		const long = `- ${'x'.repeat(MAX_PINNED_CHARS)}`;
		expect(() => writeMemoryNote(profile.slug, 'core', long)).toThrow('a pinned note');
		expect(() => addMemoryFact(profile.slug, 'core', 'x'.repeat(MAX_PINNED_CHARS))).toThrow(
			'a pinned note'
		);
		writeMemoryNote(profile.slug, 'family', long);
		expect(() => renameMemoryNote(profile.slug, 'family', 'core')).toThrow('a pinned note');
		expect(readMemoryNote(profile.slug, 'family').text).toBe(`${long}\n`);
	});

	it('is cut at a line in the prompt when it grew too long in an editor', () => {
		const { profile } = makeFamily();
		addMemoryFact(profile.slug, 'core', 'Mia is allergic to nuts');
		const line = `- ${'y'.repeat(99)}\n`;
		const text = `# Core\n\n${line.repeat(Math.ceil(MAX_PINNED_CHARS / line.length) + 5)}`;
		writeFileSync(join(profileMemoryDir(profile.slug), 'core.md'), text);

		const pinned = readPinnedNote(profile.slug, 'core.md')!;
		expect(pinned.cut).toBe(true);
		expect(pinned.text.length).toBeLessThanOrEqual(MAX_PINNED_CHARS);
		expect(text.startsWith(`${pinned.text}\n`)).toBe(true);
		expect(buildSystemPrompt(profile)).toContain('the rest was cut off here');
	});

	it('can be started on the Memory page, unless btw started it first', () => {
		const { profile } = makeFamily();
		writeMemoryFile(profile.slug, 'core.md', '- We speak Russian at home\n', 0);
		expect(readMemoryNote(profile.slug, 'core').text).toBe('- We speak Russian at home\n');

		expect(() => writeMemoryFile(profile.slug, 'core.md', '- Mine\n', 0)).toThrow(
			MemoryConflictError
		);
		const basedOn = statSync(join(profileMemoryDir(profile.slug), 'core.md')).mtimeMs;
		writeMemoryFile(profile.slug, 'core.md', '- Mine\n', basedOn);
		expect(readMemoryNote(profile.slug, 'core').text).toBe('- Mine\n');
	});
});
