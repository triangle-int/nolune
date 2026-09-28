import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { articleBefore } from './articles.ts';
import { IMAGE_SHAPES, type ImageShape } from './image-generation.ts';
import { paths } from './paths.ts';
import { isValidSkillName, parseFrontmatter } from './skills.ts';

/*
 * Image templates: the Images page's reusable starting points, like "Storybook page" or "Sticker pack". Each
 * is a folder with a TEMPLATE.md: YAML frontmatter for its card, its sentence and its settings,
 * then the prompt, where `{{setting}}` is replaced by what was picked. Applying a template sends
 * one message to a new chat: the finished prompt, with the pictures attached. The agent passes it
 * to `nolune generate image`, which knows nothing about templates. Like skills, templates come from
 * nolune itself, from ~/.nolune/image-templates (every profile) and from the profile's own
 * image-templates folder, and a template overrides one with the same id from a source before it.
 */

export type TemplateScope = 'profile' | 'global' | 'builtin';

export interface TemplateOption {
	value: string;
	label: string;
	/** What `{{setting}}` becomes in the prompt. Defaults to the label. */
	prompt: string;
}

export type TemplateSetting =
	| {
			type: 'select';
			id: string;
			label: string;
			options: TemplateOption[];
			default: string;
			/**
			 * Whether the person can type their own choice instead, and what `{{setting}}` becomes
			 * then: `{{setting}}` in it is what they typed ("Style: {{style}}."). Null when they can't.
			 */
			custom: string | null;
	  }
	| {
			type: 'text';
			id: string;
			label: string;
			placeholder: string | null;
			required: boolean;
			default: string;
	  }
	/** A few emoji, picked with the Images page's emoji picker. */
	| { type: 'emoji'; id: string; label: string; max: number; default: string };

export interface ImageTemplate {
	/** The folder name. */
	id: string;
	/** On its card. */
	name: string;
	/** The heading when it's opened, like "Catch the culprit". Defaults to the name. */
	title: string;
	/** One sentence for the family. */
	description: string;
	/**
	 * What it makes, as the sentence that opens the message: "Create a {{style}} sticker pack based
	 * on {{image}}." The Images page shows it with a chip for each setting in its place; in the
	 * message a choice becomes its label and `{{image}}` "the attached picture". Sections work as
	 * in the prompt. Settings it leaves out are shown below it.
	 */
	sentence: string | null;
	/** The tab it's listed under on the Images page. */
	category: string;
	/** A Lucide icon name and a hex color for its card, when there's no cover picture. */
	icon: string | null;
	color: string | null;
	/** Whether it starts from a picture the person picks. */
	image: 'required' | 'optional' | 'none';
	/** What to pick, e.g. "Photo of the room". */
	imageLabel: string | null;
	/** Where the picture comes from: a photo, or a drawing (made on the page, or a photo of one). */
	imageSource: 'photo' | 'drawing';
	maxImages: number;
	/** The shape it starts with; the message says it in words, like "Make it square (1:1)." */
	size: ImageShape;
	settings: TemplateSetting[];
	/** The instructions after the sentence, with `{{setting}}` placeholders. */
	prompt: string;
	/** Lower comes first within its category. */
	order: number;
	/** Absolute path to TEMPLATE.md. */
	location: string;
	/** cover.png, .jpg or .webp next to TEMPLATE.md. */
	cover: string | null;
	scope: TemplateScope;
}

/**
 * Where templates go when they don't name a category, the built-in ones included. The Images page
 * shows tabs only when templates name more than one; this one comes first.
 */
export const DEFAULT_TEMPLATE_CATEGORY = 'Templates';

const COVER_FILES = ['cover.png', 'cover.jpg', 'cover.jpeg', 'cover.webp'];
const HEX_COLOR = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;
const ICON_NAME = /^[a-z0-9-]{1,64}$/;
const PLACEHOLDER = /\{\{\s*([\w-]+)\s*\}\}/g;
const SECTION = /\{\{([#^])\s*([\w-]+)\s*\}\}([\s\S]*?)\{\{\/\s*\2\s*\}\}/g;
/** The placeholder that is set when pictures were given. */
const IMAGE_VAR = 'image';
/**
 * The chosen shape in words ("square (1:1)"), empty for auto. A prompt that uses it says the shape
 * where it wants; others get a "Make it square (1:1)." line at the end.
 */
const ASPECT_VAR = 'aspect';
const USES_ASPECT = /\{\{\s*[#^]?\s*aspect\s*\}\}/;

function slugify(label: string): string {
	return label
		.toLowerCase()
		.normalize('NFKD')
		.replace(/[^\w\s-]/g, '')
		.trim()
		.replace(/[\s_-]+/g, '-');
}

function text(value: unknown): string | null {
	if (typeof value === 'number') return String(value);
	return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function oneOf<T extends string>(value: unknown, allowed: readonly T[], what: string): T | null {
	const v = text(value)?.toLowerCase();
	if (v === undefined || v === null) return null;
	if (!(allowed as readonly string[]).includes(v)) {
		throw new Error(`${what} must be one of ${allowed.join(', ')}`);
	}
	return v as T;
}

function parseOption(raw: unknown, setting: string): TemplateOption {
	if (typeof raw === 'string' || typeof raw === 'number') {
		const label = String(raw).trim();
		return { value: slugify(label) || label, label, prompt: label };
	}
	const o = (raw ?? {}) as Record<string, unknown>;
	const label = text(o.label) ?? text(o.value);
	if (!label) throw new Error(`an option of "${setting}" has no label`);
	return {
		// Labels without letters (emoji) are their own value.
		value: text(o.value) ?? (slugify(label) || label),
		label,
		prompt: text(o.prompt) ?? label
	};
}

function parseSetting(raw: unknown): TemplateSetting {
	const s = (raw ?? {}) as Record<string, unknown>;
	const label = text(s.label) ?? text(s.id);
	if (!label) throw new Error('a setting has no label');
	const id = text(s.id) ?? slugify(label);
	if (!/^[\w-]+$/.test(id) || id === IMAGE_VAR || id === ASPECT_VAR) {
		throw new Error(`invalid setting id "${id}"`);
	}
	if (s.type === 'emoji') {
		const max = Number(s.max ?? 4);
		if (!Number.isInteger(max) || max < 1 || max > 12) {
			throw new Error(`"${label}" max must be a whole number from 1 to 12`);
		}
		const value = text(s.default) ?? '';
		const emoji = splitEmoji(value);
		if (!emoji || emoji.length > max) {
			throw new Error(`"${label}" default must be at most ${max} emoji`);
		}
		return { type: 'emoji', id, label, max, default: emoji.join('') };
	}
	if (Array.isArray(s.options)) {
		const options = s.options.map((o) => parseOption(o, label));
		if (!options.length) throw new Error(`"${label}" has no options`);
		const values = new Set(options.map((o) => o.value.toLowerCase()));
		if (values.size < options.length) throw new Error(`two options of "${label}" are the same`);
		const wanted = text(s.default);
		const first = options.find((o) => wanted && matches(o, wanted)) ?? options[0];
		const own = `{{${id}}}`;
		const custom = s.custom === true ? own : s.custom ? text(s.custom) : null;
		if (custom !== null && !custom.includes(own)) {
			throw new Error(`"${label}" custom must say where the typed choice goes, with ${own}`);
		}
		return { type: 'select', id, label, options, default: first.value, custom };
	}
	return {
		type: 'text',
		id,
		label,
		placeholder: text(s.placeholder),
		required: s.required === true,
		default: text(s.default) ?? ''
	};
}

/** A string's emoji, one per grapheme, or null if it has anything else in it (spaces aside). */
export function splitEmoji(text: string): string[] | null {
	const emoji: string[] = [];
	for (const { segment } of new Intl.Segmenter('en', { granularity: 'grapheme' }).segment(text)) {
		if (!segment.trim()) continue;
		if (!/\p{Extended_Pictographic}|\p{Regional_Indicator}|\u20e3/u.test(segment)) return null;
		emoji.push(segment);
	}
	return emoji;
}

/** How long a choice the person types can be. */
const MAX_CUSTOM = 120;

function matches(option: TemplateOption, input: string): boolean {
	const v = input.trim().toLowerCase();
	return option.value.toLowerCase() === v || option.label.toLowerCase() === v;
}

function parseTemplate(
	location: string,
	id: string,
	scope: TemplateScope,
	warnings: string[]
): ImageTemplate | null {
	const source = readFileSync(location, 'utf8');
	const meta = parseFrontmatter(source);
	if (!meta) {
		warnings.push(`${location}: frontmatter missing or unparseable, skipped`);
		return null;
	}
	const prompt = source.replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '').trim();
	try {
		const name = text(meta.name) ?? id;
		const image = oneOf(meta.image, ['required', 'optional', 'none'] as const, 'image') ?? 'none';
		const settings = Array.isArray(meta.settings) ? meta.settings.map(parseSetting) : [];
		const ids = new Set<string>();
		for (const s of settings) {
			if (ids.has(s.id)) throw new Error(`two settings are called "${s.id}"`);
			ids.add(s.id);
		}
		if (!prompt) throw new Error('the prompt (after the frontmatter) is empty');
		const sentence = text(meta.sentence);
		for (const [, key] of [
			...prompt.matchAll(PLACEHOLDER),
			...(sentence ?? '').matchAll(PLACEHOLDER)
		]) {
			if (key !== IMAGE_VAR && key !== ASPECT_VAR && !ids.has(key)) {
				warnings.push(`${location}: {{${key}}} is not one of its settings`);
			}
		}
		const color = text(meta.color);
		const icon = text(meta.icon);
		const maxImages = Number(meta['max-images'] ?? (image === 'none' ? 0 : 1));
		const dir = join(location, '..');
		return {
			id,
			name,
			title: text(meta.title) ?? name,
			description: text(meta.description) ?? '',
			sentence,
			category: text(meta.category) ?? DEFAULT_TEMPLATE_CATEGORY,
			icon: icon && ICON_NAME.test(icon) ? icon : null,
			// Goes into a style attribute, so only a plain hex color.
			color: color && HEX_COLOR.test(color) ? color : null,
			image,
			imageLabel: text(meta['image-label']),
			imageSource:
				oneOf(meta['image-source'], ['photo', 'drawing'] as const, 'image-source') ?? 'photo',
			maxImages: Number.isInteger(maxImages) && maxImages >= 0 ? maxImages : 1,
			size: oneOf(meta.size, IMAGE_SHAPES, 'size') ?? 'auto',
			settings,
			prompt,
			order: Number.isFinite(Number(meta.order)) ? Number(meta.order) : 100,
			location,
			cover: COVER_FILES.map((f) => join(dir, f)).find((f) => existsSync(f)) ?? null,
			scope
		};
	} catch (err) {
		warnings.push(`${location}: ${(err as Error).message}, skipped`);
		return null;
	}
}

function scanDir(dir: string, scope: TemplateScope, warnings: string[]): ImageTemplate[] {
	let entries;
	try {
		entries = readdirSync(dir, { withFileTypes: true });
	} catch {
		return [];
	}
	return entries.flatMap((entry) => {
		if (!entry.isDirectory() || entry.name.startsWith('.')) return [];
		const location = join(dir, entry.name, 'TEMPLATE.md');
		if (!existsSync(location)) return [];
		if (!isValidSkillName(entry.name)) {
			warnings.push(`${location}: folder names use lowercase letters, digits and hyphens, skipped`);
			return [];
		}
		return parseTemplate(location, entry.name, scope, warnings) ?? [];
	});
}

function categoryRank(category: string): number {
	return category === DEFAULT_TEMPLATE_CATEGORY ? 0 : 1;
}

/**
 * Every template a profile can use (built-in and global ones too when `profileTemplatesDir` is
 * null), sorted by category, then order, then name.
 */
export function scanImageTemplates(profileTemplatesDir: string | null): {
	templates: ImageTemplate[];
	warnings: string[];
} {
	const warnings: string[] = [];
	const byId = new Map<string, ImageTemplate>();
	const sources: [string | null, TemplateScope][] = [
		[paths.builtinImageTemplates, 'builtin'],
		[paths.globalImageTemplates, 'global'],
		[profileTemplatesDir, 'profile']
	];
	for (const [dir, scope] of sources) {
		if (!dir) continue;
		for (const template of scanDir(dir, scope, warnings)) byId.set(template.id, template);
	}
	const templates = [...byId.values()].sort(
		(a, b) =>
			categoryRank(a.category) - categoryRank(b.category) ||
			a.category.localeCompare(b.category) ||
			a.order - b.order ||
			a.name.localeCompare(b.name)
	);
	return { templates, warnings };
}

export interface TemplateChoice {
	setting: TemplateSetting;
	/** The option's value, or the text typed. Empty: not set. */
	value: string;
	/** What people see: the option's label, or the text. */
	display: string;
	/** What goes into the prompt. */
	prompt: string;
}

export interface ResolvedTemplate {
	template: ImageTemplate;
	choices: TemplateChoice[];
}

/**
 * Applies what was picked, keyed by setting id or label (`style=swiss`, `Style=Swiss`), with
 * select values matched to an option's value or label, ignoring case. Unset settings get their
 * default. Throws with a message that says what's possible.
 */
export function resolveImageTemplate(
	template: ImageTemplate,
	values: Record<string, string>
): ResolvedTemplate {
	const given = new Map<TemplateSetting, string>();
	for (const [key, value] of Object.entries(values)) {
		const k = key.trim().toLowerCase();
		const setting = template.settings.find(
			(s) => s.id.toLowerCase() === k || s.label.toLowerCase() === k
		);
		if (!setting) {
			const known = template.settings.map((s) => s.id).join(', ') || 'none';
			throw new Error(`The ${template.name} template has no setting "${key}". Settings: ${known}.`);
		}
		given.set(setting, value.trim());
	}

	const choices = template.settings.map((setting): TemplateChoice => {
		const input = given.get(setting);
		if (setting.type === 'text') {
			const value = input ?? setting.default;
			if (setting.required && !value) {
				throw new Error(`The ${template.name} template needs "${setting.label}".`);
			}
			return { setting, value, display: value, prompt: value };
		}
		if (setting.type === 'emoji') {
			const emoji = splitEmoji(input ?? setting.default);
			if (!emoji || emoji.length > setting.max) {
				throw new Error(`"${setting.label}" takes up to ${setting.max} emoji, and nothing else.`);
			}
			const value = emoji.join('');
			return { setting, value, display: value, prompt: value };
		}
		const option = input
			? setting.options.find((o) => matches(o, input))
			: setting.options.find((o) => o.value === setting.default);
		if (!option && input && setting.custom !== null) {
			// Their own choice, on one line, where the template says typed choices go.
			const own = input.replace(/\s+/g, ' ');
			if (own.length > MAX_CUSTOM) {
				throw new Error(`Keep "${setting.label}" under ${MAX_CUSTOM} characters.`);
			}
			const prompt = fillTemplate(setting.custom, { [setting.id]: own });
			return { setting, value: own, display: own, prompt };
		}
		if (!option) {
			const labels = setting.options.map((o) => o.label).join(', ');
			throw new Error(`"${input}" isn't a choice for ${setting.label}. Choices: ${labels}.`);
		}
		return { setting, value: option.value, display: option.label, prompt: option.prompt };
	});
	return { template, choices };
}

/** Throws when the pictures given don't fit the template. */
export function checkTemplateImages(template: ImageTemplate, count: number): void {
	if (template.image === 'required' && count === 0) {
		const what = template.imageLabel ? ` (${template.imageLabel.toLowerCase()})` : '';
		throw new Error(
			`The ${template.name} template starts from a picture${what}: choose one first.`
		);
	}
	if (count > Math.max(template.maxImages, template.image === 'none' ? 0 : 1)) {
		throw new Error(
			template.maxImages
				? `The ${template.name} template takes at most ${template.maxImages} picture${template.maxImages === 1 ? '' : 's'}.`
				: `The ${template.name} template doesn't use pictures.`
		);
	}
}

/**
 * Fills in `{{key}}` from `vars`. `{{#key}}…{{/key}}` is kept only when the key has a value,
 * `{{^key}}…{{/key}}` only when it doesn't, and a line that held only sections left out goes away
 * instead of staying blank.
 */
export function fillTemplate(source: string, vars: Record<string, string>): string {
	const GONE = '\u0000';
	let text = source;
	for (let previous = ''; previous !== text;) {
		previous = text;
		text = text.replace(SECTION, (_, kind: string, key: string, body: string) =>
			(kind === '#') === Boolean(vars[key]) ? body : GONE
		);
	}
	return text
		.replace(
			/(^|[^\w])([aA] )(?=\{\{\s*([\w-]+)\s*\}\})/g,
			(_, before: string, a: string, key: string) => before + articleBefore(a, vars[key] ?? '')
		)
		.replace(PLACEHOLDER, (_, key: string) => vars[key] ?? '')
		.split('\n')
		.filter((line) => !line.includes(GONE) || line.replaceAll(GONE, '').trim())
		.map((line) => line.replaceAll(GONE, '').trimEnd())
		.join('\n')
		.replace(/\n{3,}/g, '\n\n')
		.trim();
}

/** How the message names each shape; the skill maps these words back to `--size`. */
const SHAPE_WORDS: Record<ImageShape, string | null> = {
	square: 'square (1:1)',
	portrait: 'portrait (2:3)',
	landscape: 'landscape (3:2)',
	auto: null
};

/**
 * What applying a template sends: a finished prompt the family can read in the chat. The sentence
 * with the choices' labels (and "the attached picture" for `{{image}}`), then the template's
 * instructions with the choices' prompts, the shape in words, and what the person typed.
 */
export function templateMessage(
	resolved: ResolvedTemplate,
	options: { shape: ImageShape; images: number; extra?: string }
): string {
	const { template, choices } = resolved;
	const pictures =
		options.images === 0
			? ''
			: options.images === 1
				? 'the attached picture'
				: 'the attached pictures';
	const opening = template.sentence
		? fillTemplate(template.sentence, {
				[IMAGE_VAR]: pictures,
				...Object.fromEntries(choices.map((c) => [c.setting.id, c.display]))
			})
		: '';
	const shape = SHAPE_WORDS[options.shape];
	const body = fillTemplate(template.prompt, {
		[IMAGE_VAR]: pictures,
		[ASPECT_VAR]: shape ?? '',
		...Object.fromEntries(choices.map((c) => [c.setting.id, c.prompt]))
	});
	// A prompt that says the shape itself ("a {{aspect}} sticker sheet") doesn't get it again.
	const shapeLine = shape && !USES_ASPECT.test(template.prompt) ? `Make it ${shape}.` : null;
	return [opening, shapeLine ? `${body}\n${shapeLine}` : body, options.extra?.trim()]
		.filter(Boolean)
		.join('\n\n');
}
