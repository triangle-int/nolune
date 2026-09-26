import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
	checkTemplateImages,
	fillTemplate,
	resolveImageTemplate,
	scanImageTemplates,
	templateMessage,
	type ImageTemplate
} from './image-templates.ts';

function template(overrides: Partial<ImageTemplate> = {}): ImageTemplate {
	return {
		id: 'stickers',
		name: 'Sticker pack',
		title: 'Make a sticker pack',
		description: '',
		sentence:
			'Create a {{style}} sticker pack{{#image}} based on {{image}}{{/image}}{{#remix}}, remixing with {{remix}}{{/remix}}.',
		category: 'Templates',
		icon: null,
		color: null,
		image: 'optional',
		imageLabel: 'Photo to start from',
		imageSource: 'photo',
		maxImages: 3,
		size: 'square',
		settings: [
			{
				type: 'select',
				id: 'style',
				label: 'Style',
				default: '3d',
				options: [
					{ value: '3d', label: '3D', prompt: 'Use rounded, toy-like forms.' },
					{ value: 'pixel', label: 'Pixel Art', prompt: 'Use crisp square pixels.' }
				]
			},
			{
				type: 'text',
				id: 'remix',
				label: 'Remix with',
				placeholder: '💀🍓',
				required: false,
				default: ''
			}
		],
		prompt: [
			'{{style}}',
			'{{#image}}Keep the subject of the picture recognizable.{{/image}}',
			'Nine distinct stickers in a 3×3 grid on a transparent background.'
		].join('\n'),
		order: 1,
		location: '/templates/stickers/TEMPLATE.md',
		cover: null,
		scope: 'builtin',
		...overrides
	};
}

describe('resolveImageTemplate', () => {
	it('uses the defaults when nothing is picked', () => {
		const resolved = resolveImageTemplate(template(), {});
		expect(resolved.choices.map((c) => [c.setting.id, c.value])).toEqual([
			['style', '3d'],
			['remix', '']
		]);
	});

	it('matches settings and choices by id or label, ignoring case', () => {
		const resolved = resolveImageTemplate(template(), { STYLE: 'pixel art', 'Remix with': ' 💀 ' });
		expect(resolved.choices.map((c) => c.display)).toEqual(['Pixel Art', '💀']);
	});

	it('refuses unknown settings and choices, saying what there is', () => {
		expect(() => resolveImageTemplate(template(), { color: 'red' })).toThrow(
			'Settings: style, remix'
		);
		expect(() => resolveImageTemplate(template(), { style: 'Gothic' })).toThrow(
			'Choices: 3D, Pixel Art'
		);
	});

	it('requires required text', () => {
		const t = template();
		t.settings[1] = { ...t.settings[1], required: true } as ImageTemplate['settings'][number];
		expect(() => resolveImageTemplate(t, {})).toThrow('needs "Remix with"');
	});
});

describe('fillTemplate', () => {
	const source = 'A.\n{{#x}}Has {{x}}.{{/x}}\n{{^x}}No x.{{/x}}\nB {{#y}}and {{y}}{{/y}}.';

	it('keeps the sections that apply', () => {
		expect(fillTemplate(source, { x: 'one', y: 'two' })).toBe('A.\nHas one.\nB and two.');
	});

	it('drops lines that only held sections left out, not other lines', () => {
		expect(fillTemplate(source, {})).toBe('A.\nNo x.\nB .');
	});
});

describe('templateMessage', () => {
	it('is the sentence with labels, then the instructions, the shape and extra wishes', () => {
		const resolved = resolveImageTemplate(template(), { remix: '💀🍓🛼💨' });
		expect(
			templateMessage(resolved, { shape: 'square', images: 2, extra: ' Make it pink. ' })
		).toBe(
			[
				'Create a 3D sticker pack based on the attached pictures, remixing with 💀🍓🛼💨.',
				'',
				'Use rounded, toy-like forms.',
				'Keep the subject of the picture recognizable.',
				'Nine distinct stickers in a 3×3 grid on a transparent background.',
				'Make it square (1:1).',
				'',
				'Make it pink.'
			].join('\n')
		);
	});

	it('leaves out what was not given', () => {
		const resolved = resolveImageTemplate(template(), { style: 'pixel' });
		expect(templateMessage(resolved, { shape: 'auto', images: 0 })).toBe(
			[
				'Create a Pixel Art sticker pack.',
				'',
				'Use crisp square pixels.',
				'Nine distinct stickers in a 3×3 grid on a transparent background.'
			].join('\n')
		);
	});

	it('says the shape where the prompt asks for it, instead of at the end', () => {
		const t = template({ prompt: 'A single {{#aspect}}{{aspect}} {{/aspect}}sticker sheet.' });
		expect(templateMessage(resolveImageTemplate(t, {}), { shape: 'square', images: 0 })).toBe(
			'Create a 3D sticker pack.\n\nA single square (1:1) sticker sheet.'
		);
		expect(templateMessage(resolveImageTemplate(t, {}), { shape: 'auto', images: 0 })).toBe(
			'Create a 3D sticker pack.\n\nA single sticker sheet.'
		);
	});

	it('is just the instructions without a sentence', () => {
		const resolved = resolveImageTemplate(template({ sentence: null }), {});
		expect(templateMessage(resolved, { shape: 'portrait', images: 1 })).toMatch(
			/^Use rounded, toy-like forms\.\n.*\nMake it portrait \(2:3\)\.$/s
		);
	});
});

describe('scanImageTemplates', () => {
	const dir = mkdtempSync(join(tmpdir(), 'btw-templates-'));
	afterAll(() => rmSync(dir, { recursive: true, force: true }));

	function write(id: string, frontmatter: string): void {
		mkdirSync(join(dir, 'image-templates', id), { recursive: true });
		writeFileSync(
			join(dir, 'image-templates', id, 'TEMPLATE.md'),
			`---\nname: ${id}\n${frontmatter}\n---\n\nA sticker of {{remix}}.\n`
		);
	}

	it('keeps emoji-only choices apart, each its own value', () => {
		write('emoji', 'settings:\n  - id: remix\n    options:\n      - 💀🍓\n      - 🌻💥');
		const { templates } = scanImageTemplates(join(dir, 'image-templates'));
		const remix = templates.find((t) => t.id === 'emoji')!.settings[0];
		expect(remix.type === 'select' && remix.options.map((o) => o.value)).toEqual(['💀🍓', '🌻💥']);
		const resolved = resolveImageTemplate(
			templates.find((t) => t.id === 'emoji')!,
			{
				remix: '🌻💥'
			}
		);
		expect(resolved.choices[0].prompt).toBe('🌻💥');
	});

	it("skips a template whose setting takes a name btw fills in, or whose choices can't be told apart", () => {
		write('aspect', 'settings:\n  - id: aspect\n    label: Aspect');
		write('twins', 'settings:\n  - id: remix\n    options:\n      - Pink!\n      - pink');
		const { templates, warnings } = scanImageTemplates(join(dir, 'image-templates'));
		expect(templates.map((t) => t.id)).not.toContain('aspect');
		expect(templates.map((t) => t.id)).not.toContain('twins');
		expect(warnings.join('\n')).toMatch(/invalid setting id "aspect"/);
		expect(warnings.join('\n')).toMatch(/two options of "remix" are the same/);
	});
});

describe('checkTemplateImages', () => {
	it('asks for a required picture', () => {
		expect(() => checkTemplateImages(template({ image: 'required' }), 0)).toThrow(
			'starts from a picture'
		);
	});

	it('refuses more pictures than the template takes', () => {
		expect(() => checkTemplateImages(template(), 4)).toThrow('at most 3 pictures');
		expect(() => checkTemplateImages(template({ image: 'none', maxImages: 0 }), 1)).toThrow(
			"doesn't use pictures"
		);
		expect(() => checkTemplateImages(template(), 3)).not.toThrow();
	});
});

describe('the built-in templates', () => {
	const { templates, warnings } = scanImageTemplates(null);

	it('all parse, without warnings', () => {
		expect(warnings).toEqual([]);
		expect(templates.length).toBeGreaterThanOrEqual(15);
	});

	it('share one grid, in a fixed order', () => {
		expect(new Set(templates.map((t) => t.category))).toEqual(new Set(['Templates']));
		const orders = templates.map((t) => t.order);
		expect(new Set(orders).size, 'each built-in has its own place').toBe(orders.length);
	});

	it('make a finished message with their defaults', () => {
		for (const t of templates) {
			const message = templateMessage(resolveImageTemplate(t, requiredText(t)), {
				shape: t.size,
				images: t.image === 'none' ? 0 : 1
			});
			expect(message, t.id).not.toMatch(/\{\{|\}\}|\s[.,]/);
		}
	});

	it('give every setting a place in their sentence', () => {
		for (const t of templates.filter((t) => t.sentence)) {
			for (const setting of t.settings) {
				expect(t.sentence, `${t.id}: ${setting.id}`).toContain(`{{${setting.id}}}`);
			}
		}
	});
});

/** Something for each required text setting, as a person would type. */
function requiredText(t: ImageTemplate): Record<string, string> {
	return Object.fromEntries(
		t.settings.flatMap((s) => (s.type === 'text' && s.required ? [[s.id, 'Something']] : []))
	);
}
