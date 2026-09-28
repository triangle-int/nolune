import { describe, expect, it } from 'vitest';
import { countSections, parseMemoryExport, projectName } from './memory-export.ts';

const REPLY = `Here's everything I have stored about you:

\`\`\`
## Instructions
[2025-03-02] - Keep answers short. Skip the preamble.
[2025-06-18] - Show code in Rust unless I say otherwise.

## Identity
[unknown] - Indie game developer.
[2024-11-09] - Speaks English and Russian.

## Career
[2023-01-15] - Gameplay programmer at a small studio.

## Projects
[2026-01-12] - Tidepool: a Godot farming game, in beta.

## Preferences
[unknown] - Prefers dark themes.
\`\`\`

This is the complete set.`;

describe('parseMemoryExport', () => {
	it('reads the dated lines in the code block, section by section', () => {
		const facts = parseMemoryExport(REPLY);
		expect(facts).toEqual([
			{
				section: 'instructions',
				text: 'Keep answers short. Skip the preamble.',
				date: '2025-03-02'
			},
			{
				section: 'instructions',
				text: 'Show code in Rust unless I say otherwise.',
				date: '2025-06-18'
			},
			{ section: 'identity', text: 'Indie game developer.', date: null },
			{ section: 'identity', text: 'Speaks English and Russian.', date: '2024-11-09' },
			{ section: 'career', text: 'Gameplay programmer at a small studio.', date: '2023-01-15' },
			{ section: 'projects', text: 'Tidepool: a Godot farming game, in beta.', date: '2026-01-12' },
			{ section: 'preferences', text: 'Prefers dark themes.', date: null }
		]);
	});

	it('leaves out what the assistant says around the code block', () => {
		const texts = parseMemoryExport(REPLY).map((f) => f.text);
		expect(texts).not.toContain('This is the complete set.');
		expect(texts.some((t) => t.includes('everything I have stored'))).toBe(false);
	});

	it('takes other heading styles, list markers and dashes', () => {
		const facts = parseMemoryExport(
			[
				'1. **Instructions**:',
				'- [2025-01-02] — Answer in British English.',
				'**Projects**',
				'* [unknown]: Lumen - a Rust ECS.',
				'Preferences:',
				'• [2024-05-05] - Likes cats.'
			].join('\n')
		);
		expect(facts).toEqual([
			{ section: 'instructions', text: 'Answer in British English.', date: '2025-01-02' },
			{ section: 'projects', text: 'Lumen - a Rust ECS.', date: null },
			{ section: 'preferences', text: 'Likes cats.', date: '2024-05-05' }
		]);
	});

	it('only counts dated lines and list items outside a code block', () => {
		const facts = parseMemoryExport(
			'## Identity\n[unknown] - Lives in Berlin.\nI hope this helps!\n- Has two kids.'
		);
		expect(facts.map((f) => f.text)).toEqual(['Lives in Berlin.', 'Has two kids.']);
	});

	it('keeps lines under an unknown heading as other, and skips lines before any heading', () => {
		const facts = parseMemoryExport(
			'```\nstray line\n## Health\n[unknown] - Allergic to nuts.\n```'
		);
		expect(facts).toEqual([{ section: 'other', text: 'Allergic to nuts.', date: null }]);
	});

	it('drops dates that are not real days, or are in the future', () => {
		const facts = parseMemoryExport(
			'```\n## Identity\n[2025-02-30] - A.\n[2999-01-01] - B.\n[last year] - C.\n```'
		);
		expect(facts.map((f) => [f.text, f.date])).toEqual([
			['A.', null],
			['B.', null],
			['C.', null]
		]);
	});

	it('finds nothing in text that is not an export', () => {
		expect(parseMemoryExport('I don’t have any memories stored about you yet.')).toEqual([]);
	});
});

describe('countSections', () => {
	it('counts every section, empty ones as 0', () => {
		expect(countSections(parseMemoryExport(REPLY))).toEqual({
			instructions: 2,
			identity: 2,
			career: 1,
			projects: 1,
			preferences: 1,
			other: 0
		});
	});
});

describe('projectName', () => {
	it.each([
		['Tidepool: a Godot farming game', 'Tidepool'],
		['Lumen - a Rust ECS', 'Lumen'],
		['**Nolune agent** (a family assistant)', 'Nolune agent'],
		['Built a small Rust ECS for my games', null],
		['A very long descriptor that goes on and on and on for ages: done', null]
	])('%s is %s', (text, name) => {
		expect(projectName(text)).toBe(name);
	});
});
