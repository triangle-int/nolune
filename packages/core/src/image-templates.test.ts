import { describe, expect, it } from 'vitest';
import {
	buildTemplatePrompt,
	checkTemplateImages,
	resolveImageTemplate,
	scanImageTemplates,
	templateMessage,
	type ImageTemplate
} from './image-templates.ts';

function template(overrides: Partial<ImageTemplate> = {}): ImageTemplate {
	return {
		id: 'poster',
		name: 'Poster',
		title: 'Make a poster',
		description: '',
		sentence: null,
		category: 'Templates',
		icon: null,
		color: null,
		image: 'optional',
		imageLabel: 'Photo to feature',
		imageSource: 'photo',
		maxImages: 1,
		size: 'portrait',
		quality: 'high',
		background: null,
		format: null,
		settings: [
			{
				type: 'select',
				id: 'style',
				label: 'Style',
				default: 'swiss',
				options: [
					{ value: 'swiss', label: 'Swiss', prompt: 'a Swiss grid', background: null },
					{
						value: 'sticker',
						label: 'Sticker',
						prompt: 'a die-cut sticker',
						background: 'transparent'
					}
				]
			},
			{
				type: 'text',
				id: 'headline',
				label: 'Headline',
				placeholder: 'BLOOM',
				required: false,
				default: ''
			}
		],
		prompt: [
			'A poster.',
			'{{#image}}Built around the attached picture.{{/image}}',
			'Style: {{style}}.',
			'{{#headline}}Headline: "{{headline}}".{{/headline}}',
			'{{^headline}}No text.{{/headline}}'
		].join('\n'),
		order: 1,
		location: '/templates/poster/TEMPLATE.md',
		cover: null,
		scope: 'builtin',
		...overrides
	};
}

describe('resolveImageTemplate', () => {
	it('uses the defaults when nothing is picked', () => {
		const resolved = resolveImageTemplate(template(), {});
		expect(resolved.choices.map((c) => [c.setting.id, c.value])).toEqual([
			['style', 'swiss'],
			['headline', '']
		]);
		expect(resolved.background).toBeNull();
	});

	it('matches settings and choices by id or label, ignoring case', () => {
		const resolved = resolveImageTemplate(template(), { STYLE: 'sticker', Headline: ' BLOOM ' });
		expect(resolved.choices.map((c) => c.display)).toEqual(['Sticker', 'BLOOM']);
	});

	it("takes an option's background", () => {
		expect(resolveImageTemplate(template(), { style: 'Sticker' }).background).toBe('transparent');
	});

	it('refuses unknown settings and choices, saying what there is', () => {
		expect(() => resolveImageTemplate(template(), { color: 'red' })).toThrow(
			'Settings: style, headline'
		);
		expect(() => resolveImageTemplate(template(), { style: 'Gothic' })).toThrow(
			'Choices: Swiss, Sticker'
		);
	});

	it('requires required text', () => {
		const t = template();
		t.settings[1] = { ...t.settings[1], required: true } as ImageTemplate['settings'][number];
		expect(() => resolveImageTemplate(t, {})).toThrow('needs "Headline"');
	});
});

describe('buildTemplatePrompt', () => {
	it('fills in choices and keeps the sections that apply', () => {
		const resolved = resolveImageTemplate(template(), { headline: 'BLOOM' });
		expect(buildTemplatePrompt(resolved, { hasImages: true })).toBe(
			'A poster.\nBuilt around the attached picture.\nStyle: a Swiss grid.\nHeadline: "BLOOM".'
		);
	});

	it('drops the lines of sections left out instead of leaving them blank', () => {
		const resolved = resolveImageTemplate(template(), {});
		expect(buildTemplatePrompt(resolved, { hasImages: false })).toBe(
			'A poster.\nStyle: a Swiss grid.\nNo text.'
		);
	});

	it('adds what the person asked for at the end', () => {
		const resolved = resolveImageTemplate(template(), {});
		expect(buildTemplatePrompt(resolved, { hasImages: false, extra: ' make it pink ' })).toMatch(
			/No text\.\n\nAlso: make it pink$/
		);
	});
});

describe('templateMessage', () => {
	it('puts the options before the prompt', () => {
		const resolved = resolveImageTemplate(template(), { style: 'sticker' });
		expect(templateMessage(resolved, { shape: 'square', hasImages: false })).toBe(
			[
				'Make an image with the Poster template.',
				'Size: square',
				'Quality: high',
				'Background: transparent',
				'',
				'Prompt:',
				'A poster.',
				'Style: a die-cut sticker.',
				'No text.'
			].join('\n')
		);
	});
});

describe('checkTemplateImages', () => {
	it('asks for a required picture', () => {
		expect(() => checkTemplateImages(template({ image: 'required' }), 0)).toThrow(
			'starts from a picture'
		);
	});

	it('refuses more pictures than the template takes', () => {
		expect(() => checkTemplateImages(template(), 2)).toThrow('at most 1 picture');
		expect(() => checkTemplateImages(template({ image: 'none', maxImages: 0 }), 1)).toThrow(
			"doesn't use pictures"
		);
		expect(() => checkTemplateImages(template(), 1)).not.toThrow();
	});
});

describe('the built-in templates', () => {
	const { templates, warnings } = scanImageTemplates(null);

	it('all parse, without warnings', () => {
		expect(warnings).toEqual([]);
		expect(templates.length).toBeGreaterThanOrEqual(15);
	});

	it('list Templates before Trending', () => {
		const categories = [...new Set(templates.map((t) => t.category))];
		expect(categories.slice(0, 2)).toEqual(['Templates', 'Trending']);
	});

	it('build a prompt with their defaults', () => {
		for (const t of templates) {
			const prompt = buildTemplatePrompt(resolveImageTemplate(t, requiredText(t)), {
				hasImages: t.image !== 'none'
			});
			expect(prompt, t.id).not.toMatch(/\{\{|\}\}/);
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
