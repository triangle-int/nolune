import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import {
	checkTemplateImages,
	fillTemplate,
	resolveImageTemplate,
	scanImageTemplates,
	splitEmoji,
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
				custom: null,
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

	it('takes a typed choice when the setting allows one, worded as the template says', () => {
		const t = template();
		t.settings[0] = {
			...t.settings[0],
			custom: 'Style: {{style}}.'
		} as ImageTemplate['settings'][number];
		const choice = (style: string) => resolveImageTemplate(t, { style }).choices[0];
		expect(choice('  claymation\n with  googly eyes ')).toMatchObject({
			value: 'claymation with googly eyes',
			display: 'claymation with googly eyes',
			prompt: 'Style: claymation with googly eyes.'
		});
		// One of the choices, typed, is that choice.
		expect(choice('pixel art').prompt).toBe('Use crisp square pixels.');
		expect(() => choice('x'.repeat(121))).toThrow('under 120 characters');
		expect(() => resolveImageTemplate(template(), { style: 'claymation' })).toThrow(
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

	it('reads whether a choice can be typed, and how it is worded', () => {
		write('typed', 'settings:\n  - id: style\n    custom: true\n    options:\n      - pink');
		write(
			'worded',
			"settings:\n  - id: style\n    custom: 'Use a {{style}} look.'\n    options:\n      - pink"
		);
		const { templates } = scanImageTemplates(join(dir, 'image-templates'));
		const custom = (id: string) => {
			const setting = templates.find((t) => t.id === id)!.settings[0];
			return setting.type === 'select' ? setting.custom : undefined;
		};
		expect(custom('typed')).toBe('{{style}}');
		expect(custom('worded')).toBe('Use a {{style}} look.');
		expect(custom('emoji')).toBe(null);
	});

	it('reads emoji settings, with their default and how many they hold', () => {
		write('picker', 'settings:\n  - id: remix\n    type: emoji\n    max: 3\n    default: 🌻💥');
		const { templates } = scanImageTemplates(join(dir, 'image-templates'));
		expect(templates.find((t) => t.id === 'picker')!.settings[0]).toEqual({
			type: 'emoji',
			id: 'remix',
			label: 'remix',
			max: 3,
			default: '🌻💥'
		});
	});

	it("skips a template whose setting takes a name btw fills in, or whose choices can't be told apart", () => {
		write('aspect', 'settings:\n  - id: aspect\n    label: Aspect');
		write('words', 'settings:\n  - id: remix\n    type: emoji\n    default: pink');
		write(
			'nowhere',
			"settings:\n  - id: style\n    custom: 'Your own look.'\n    options:\n      - pink"
		);
		write('twins', 'settings:\n  - id: remix\n    options:\n      - Pink!\n      - pink');
		const { templates, warnings } = scanImageTemplates(join(dir, 'image-templates'));
		expect(templates.map((t) => t.id)).not.toContain('aspect');
		expect(templates.map((t) => t.id)).not.toContain('twins');
		expect(templates.map((t) => t.id)).not.toContain('words');
		expect(templates.map((t) => t.id)).not.toContain('nowhere');
		expect(warnings.join('\n')).toMatch(/invalid setting id "aspect"/);
		expect(warnings.join('\n')).toMatch(/two options of "remix" are the same/);
		expect(warnings.join('\n')).toMatch(/"remix" default must be at most 4 emoji/);
		expect(warnings.join('\n')).toMatch(/"style" custom must say where the typed choice goes/);
	});
});

describe('emoji settings', () => {
	const emoji = template({
		settings: [{ type: 'emoji', id: 'remix', label: 'Remix with', max: 4, default: '💀🍓' }]
	});

	it('count each emoji once, whatever it is made of', () => {
		expect(splitEmoji('👍🏽 🇰🇬1️⃣👨‍👩‍👧')).toEqual(['👍🏽', '🇰🇬', '1️⃣', '👨‍👩‍👧']);
		expect(splitEmoji('')).toEqual([]);
		expect(splitEmoji('🍓 and 💀')).toBe(null);
	});

	it('take what was picked without spaces, or nothing', () => {
		const pick = (remix: string) => resolveImageTemplate(emoji, { remix }).choices[0].prompt;
		expect(pick('🌻 💥 🍉')).toBe('🌻💥🍉');
		expect(pick('')).toBe('');
		expect(resolveImageTemplate(emoji, {}).choices[0].prompt).toBe('💀🍓');
	});

	it('refuse words and more emoji than they hold', () => {
		expect(() => resolveImageTemplate(emoji, { remix: 'pink' })).toThrow('up to 4 emoji');
		expect(() => resolveImageTemplate(emoji, { remix: '🌻💥🍉🎨🔥' })).toThrow('up to 4 emoji');
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

	it('make a finished message with a typed choice, where they take one', () => {
		for (const t of templates) {
			for (const s of t.settings.filter((s) => s.type === 'select' && s.custom !== null)) {
				const message = templateMessage(
					resolveImageTemplate(t, { ...requiredText(t), [s.id]: 'claymation' }),
					{ shape: t.size, images: t.image === 'none' ? 0 : 1 }
				);
				expect(message, `${t.id}: ${s.id}`).toContain('claymation');
				expect(message, t.id).not.toMatch(/\{\{|\}\}|\s[.,]/);
			}
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
