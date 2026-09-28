import { describe, expect, it } from 'vitest';
import { MAX_PINNED_CHARS, listMemoryFiles, readMemoryNote } from './memory.ts';
import { parseMemoryExport, type ExportedFact } from './memory-export.ts';
import { noteName } from './memory-categories.ts';
import { importMemoryExport } from './memory-import.ts';
import { makeFamily } from './test/fixtures.ts';

const EXPORT = `\`\`\`
## Instructions
[2025-03-02] - Keep answers short.
## Identity
[unknown] - Indie game developer.
[2024-11-09] - Speaks English and Russian.
## Career
[2023-01-15] - Gameplay programmer at a small studio.
## Projects
[2026-01-12] - Tidepool: a Godot farming game, in beta.
[2026-02-01] - Tidepool: switched to Godot 5.
[unknown] - Built a tiny Rust ECS for jams.
## Preferences
[unknown] - Prefers dark themes.
\`\`\``;

describe('importMemoryExport', () => {
	it('pins rules with the name, and files the rest by person and project', () => {
		const { profile } = makeFamily();
		const result = importMemoryExport(profile.slug, 'Jamie Doe', parseMemoryExport(EXPORT));

		expect(readMemoryNote(profile.slug, 'core').text).toBe(
			'# Core\n\n- Jamie: Keep answers short.\n'
		);
		expect(readMemoryNote(profile.slug, 'people/jamie').text).toBe(
			[
				'# Jamie',
				'',
				'## Identity',
				'',
				'- Indie game developer.',
				'- Speaks English and Russian.',
				'',
				'## Career',
				'',
				'- Gameplay programmer at a small studio.',
				'',
				'## Preferences',
				'',
				'- Prefers dark themes.',
				''
			].join('\n')
		);
		expect(readMemoryNote(profile.slug, 'projects/tidepool').text).toBe(
			'# Tidepool\n\n- Tidepool: a Godot farming game, in beta.\n- Tidepool: switched to Godot 5.\n'
		);
		expect(readMemoryNote(profile.slug, 'projects').text).toBe(
			'# Projects\n\n- Built a tiny Rust ECS for jams.\n'
		);
		expect(result.added).toBe(8);
		expect(result.notes.map((n) => n.path)).toEqual([
			'core.md',
			'people/jamie.md',
			'projects/tidepool.md',
			'projects.md'
		]);
	});

	it('dates facts as the export did, and leaves [unknown] undated', () => {
		const { profile } = makeFamily();
		importMemoryExport(profile.slug, 'Jamie', parseMemoryExport(EXPORT));
		const jamie = listMemoryFiles(profile.slug).find((f) => f.path === 'people/jamie.md')!;
		expect(jamie.facts).toEqual([
			{ text: 'Indie game developer.', learnedAt: null },
			{ text: 'Speaks English and Russian.', learnedAt: Date.UTC(2024, 10, 9) },
			{ text: 'Gameplay programmer at a small studio.', learnedAt: Date.UTC(2023, 0, 15) },
			{ text: 'Prefers dark themes.', learnedAt: null }
		]);
	});

	it('skips what memory already has, so importing twice adds nothing', () => {
		const { profile } = makeFamily();
		importMemoryExport(profile.slug, 'Jamie', parseMemoryExport(EXPORT));
		const again = importMemoryExport(profile.slug, 'Jamie', parseMemoryExport(EXPORT));
		expect(again).toEqual({ notes: [], added: 0, skipped: 8 });
	});

	it("adds to a heading that's already in the person's note", () => {
		const { profile } = makeFamily();
		importMemoryExport(profile.slug, 'Jamie', parseMemoryExport(EXPORT));
		importMemoryExport(profile.slug, 'Jamie', [
			{ section: 'identity', text: 'Lives in Lisbon.', date: null }
		]);
		expect(readMemoryNote(profile.slug, 'people/jamie').text).toContain(
			'- Speaks English and Russian.\n- Lives in Lisbon.\n\n## Career'
		);
	});

	it("moves rules that don't fit in the pinned note to the person's note", () => {
		const { profile } = makeFamily();
		const rules: ExportedFact[] = Array.from({ length: 60 }, (_, i) => ({
			section: 'instructions',
			text: `Rule number ${i} ${'x'.repeat(80)}`,
			date: null
		}));
		const result = importMemoryExport(profile.slug, 'Jamie', rules);
		const core = readMemoryNote(profile.slug, 'core').text;
		expect(core.length).toBeLessThanOrEqual(MAX_PINNED_CHARS);
		const person = readMemoryNote(profile.slug, 'people/jamie').text;
		expect(person).toContain('## Instructions');
		expect(person).toContain('- Rule number 59');
		expect(person).not.toContain('Jamie: Rule');
		expect(result.added).toBe(60);
	});
});

describe('noteName', () => {
	it.each([
		['Anna Smith', 'anna-smith'],
		['Zoë', 'zoe'],
		['Алёна', 'алена'],
		['!!!', 'someone']
	])('%s is %s', (name, note) => {
		expect(noteName(name)).toBe(note);
	});
});
