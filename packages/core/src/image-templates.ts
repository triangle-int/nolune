import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
	IMAGE_BACKGROUNDS,
	IMAGE_FORMATS,
	IMAGE_SHAPES,
	type ImageBackground,
	type ImageFormat,
	type ImageShape
} from './image-generation.ts';
import { paths } from './paths.ts';
import { isValidSkillName, parseFrontmatter } from './skills.ts';

/*
 * Image templates: the Images page's reusable starting points, like "Poster" or "Stickers". Each
 * is a folder with a TEMPLATE.md: YAML frontmatter for its card and its settings, then the prompt,
 * where `{{setting}}` is replaced by what was picked. The page turns a template and its choices
 * into a finished prompt in the message it sends; the agent passes that prompt to
 * `btw generate image`, which knows nothing about templates. Like skills, templates come from btw
 * itself, from ~/.btw-agent/image-templates (every profile) and from the profile's own
 * image-templates folder, and a template overrides one with the same id from a source before it.
 */

export type TemplateScope = 'profile' | 'global' | 'builtin';

export interface TemplateOption {
	value: string;
	label: string;
	/** What `{{setting}}` becomes in the prompt. Defaults to the label. */
	prompt: string;
	/** Overrides the template's background when picked, e.g. transparent for a sprite. */
	background: ImageBackground | null;
}

export type TemplateSetting =
	| { type: 'select'; id: string; label: string; options: TemplateOption[]; default: string }
	| {
			type: 'text';
			id: string;
			label: string;
			placeholder: string | null;
			required: boolean;
			default: string;
	  };

export interface ImageTemplate {
	/** The folder name. */
	id: string;
	name: string;
	/** One sentence for the family. */
	description: string;
	/** The tab it's listed under on the Images page. */
	category: string;
	/** A Lucide icon name and a hex color for its card, when there's no cover picture. */
	icon: string | null;
	color: string | null;
	/** Whether it starts from a picture the person picks. */
	image: 'required' | 'optional' | 'none';
	/** What to pick, e.g. "Photo of the room". */
	imageLabel: string | null;
	maxImages: number;
	size: ImageShape;
	quality: string | null;
	background: ImageBackground | null;
	format: ImageFormat | null;
	settings: TemplateSetting[];
	/** The prompt, with `{{setting}}` placeholders. */
	prompt: string;
	/** Lower comes first within its category. */
	order: number;
	/** Absolute path to TEMPLATE.md. */
	location: string;
	/** cover.png, .jpg or .webp next to TEMPLATE.md. */
	cover: string | null;
	scope: TemplateScope;
}

/** Tabs on the Images page come in this order; other categories follow, alphabetically. */
export const TEMPLATE_CATEGORIES = ['Templates', 'Trending'];
/** For templates that don't name a category. */
export const DEFAULT_TEMPLATE_CATEGORY = 'Yours';

const COVER_FILES = ['cover.png', 'cover.jpg', 'cover.jpeg', 'cover.webp'];
const HEX_COLOR = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;
const ICON_NAME = /^[a-z0-9-]{1,64}$/;
const PLACEHOLDER = /\{\{\s*([\w-]+)\s*\}\}/g;
const SECTION = /\{\{([#^])\s*([\w-]+)\s*\}\}([\s\S]*?)\{\{\/\s*\2\s*\}\}/g;
/** The placeholder that is set when pictures were given. */
const IMAGE_VAR = 'image';

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
		return { value: slugify(label), label, prompt: label, background: null };
	}
	const o = (raw ?? {}) as Record<string, unknown>;
	const label = text(o.label) ?? text(o.value);
	if (!label) throw new Error(`an option of "${setting}" has no label`);
	return {
		value: text(o.value) ?? slugify(label),
		label,
		prompt: text(o.prompt) ?? label,
		background: oneOf(o.background, IMAGE_BACKGROUNDS, `background of "${label}"`)
	};
}

function parseSetting(raw: unknown): TemplateSetting {
	const s = (raw ?? {}) as Record<string, unknown>;
	const label = text(s.label) ?? text(s.id);
	if (!label) throw new Error('a setting has no label');
	const id = text(s.id) ?? slugify(label);
	if (!/^[\w-]+$/.test(id) || id === IMAGE_VAR) throw new Error(`invalid setting id "${id}"`);
	if (Array.isArray(s.options)) {
		const options = s.options.map((o) => parseOption(o, label));
		if (!options.length) throw new Error(`"${label}" has no options`);
		const wanted = text(s.default);
		const first = options.find((o) => wanted && matches(o, wanted)) ?? options[0];
		return { type: 'select', id, label, options, default: first.value };
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
		for (const [, key] of prompt.matchAll(PLACEHOLDER)) {
			if (key !== IMAGE_VAR && !ids.has(key)) {
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
			description: text(meta.description) ?? '',
			category: text(meta.category) ?? DEFAULT_TEMPLATE_CATEGORY,
			icon: icon && ICON_NAME.test(icon) ? icon : null,
			// Goes into a style attribute, so only a plain hex color.
			color: color && HEX_COLOR.test(color) ? color : null,
			image,
			imageLabel: text(meta['image-label']),
			maxImages: Number.isInteger(maxImages) && maxImages >= 0 ? maxImages : 1,
			size: oneOf(meta.size, IMAGE_SHAPES, 'size') ?? 'auto',
			quality: text(meta.quality),
			background: oneOf(meta.background, IMAGE_BACKGROUNDS, 'background'),
			format: oneOf(meta.format, IMAGE_FORMATS, 'format'),
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
	const i = TEMPLATE_CATEGORIES.indexOf(category);
	return i === -1 ? TEMPLATE_CATEGORIES.length : i;
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
	background: ImageBackground | null;
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

	let background = template.background;
	const choices = template.settings.map((setting): TemplateChoice => {
		const input = given.get(setting);
		if (setting.type === 'text') {
			const value = input ?? setting.default;
			if (setting.required && !value) {
				throw new Error(`The ${template.name} template needs "${setting.label}".`);
			}
			return { setting, value, display: value, prompt: value };
		}
		const option = input
			? setting.options.find((o) => matches(o, input))
			: setting.options.find((o) => o.value === setting.default);
		if (!option) {
			const labels = setting.options.map((o) => o.label).join(', ');
			throw new Error(`"${input}" isn't a choice for ${setting.label}. Choices: ${labels}.`);
		}
		if (option.background) background = option.background;
		return { setting, value: option.value, display: option.label, prompt: option.prompt };
	});
	return { template, choices, background };
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
 * The template's prompt with the choices filled in. `{{#key}}…{{/key}}` is kept only when the
 * setting has a value (or, for `image`, when pictures were given), `{{^key}}…{{/key}}` only when
 * it doesn't. `extra`: what the person asked for on top, added at the end.
 */
export function buildTemplatePrompt(
	resolved: ResolvedTemplate,
	options: { hasImages: boolean; extra?: string }
): string {
	const vars: Record<string, string> = { [IMAGE_VAR]: options.hasImages ? 'yes' : '' };
	for (const choice of resolved.choices) vars[choice.setting.id] = choice.prompt;

	// A left-out section leaves a marker, so a line that held only sections disappears whole
	// instead of becoming a blank line.
	const GONE = '\u0000';
	let prompt = resolved.template.prompt;
	for (let previous = ''; previous !== prompt;) {
		previous = prompt;
		prompt = prompt.replace(SECTION, (_, kind: string, key: string, body: string) =>
			(kind === '#') === Boolean(vars[key]) ? body : GONE
		);
	}
	prompt = prompt
		.replace(PLACEHOLDER, (_, key: string) => vars[key] ?? '')
		.split('\n')
		.filter((line) => !line.includes(GONE) || line.replaceAll(GONE, '').trim())
		.map((line) => line.replaceAll(GONE, '').trimEnd())
		.join('\n')
		.replace(/\n{3,}/g, '\n\n')
		.trim();
	const extra = options.extra?.trim();
	return extra ? `${prompt}\n\nAlso: ${extra}` : prompt;
}

/**
 * The message the Images page sends: the options that aren't part of the prompt, as the flags of
 * `btw generate image` they stand for, then the finished prompt. The generate-images skill tells
 * the agent to pass both on unchanged. The family sees the same text in the chat.
 */
export function templateMessage(
	resolved: ResolvedTemplate,
	options: { shape: ImageShape; hasImages: boolean; extra?: string }
): string {
	const { template } = resolved;
	const lines = [`Make an image with the ${template.name} template.`, `Size: ${options.shape}`];
	if (template.quality) lines.push(`Quality: ${template.quality}`);
	if (resolved.background) lines.push(`Background: ${resolved.background}`);
	if (template.format) lines.push(`Format: ${template.format}`);
	return `${lines.join('\n')}\n\nPrompt:\n${buildTemplatePrompt(resolved, options)}`;
}
