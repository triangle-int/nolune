import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse as parseYaml } from 'yaml';
import { paths } from './paths.ts';

export interface Skill {
	name: string;
	description: string;
	/** Absolute path to SKILL.md. */
	location: string;
	scope: 'profile' | 'global';
}

const SKIP_DIRS = new Set(['.git', 'node_modules']);
const MAX_DEPTH = 4;

/** Per the Agent Skills spec: lowercase letters, digits and single hyphens, max 64 chars. */
export function isValidSkillName(name: string): boolean {
	return name.length <= 64 && /^[a-z0-9]+(-[a-z0-9]+)*$/.test(name);
}

function parseFrontmatter(text: string): Record<string, unknown> | null {
	const match = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(text);
	if (!match) return null;
	try {
		const data = parseYaml(match[1]);
		return data && typeof data === 'object' ? (data as Record<string, unknown>) : null;
	} catch {
		// Skills written for other clients often have unquoted colons in values; quote and retry.
		const fixed = match[1].replace(
			/^(\s*[\w-]+):\s+(?!["'|>])(.*:.*)$/gm,
			(_, key: string, value: string) => `${key}: ${JSON.stringify(value)}`
		);
		try {
			const data = parseYaml(fixed);
			return data && typeof data === 'object' ? (data as Record<string, unknown>) : null;
		} catch {
			return null;
		}
	}
}

function scanDir(
	root: string,
	scope: Skill['scope'],
	out: Skill[],
	warnings: string[],
	dir = root,
	depth = 0
): void {
	let entries;
	try {
		entries = readdirSync(dir, { withFileTypes: true });
	} catch {
		return;
	}
	for (const entry of entries) {
		if (!entry.isDirectory() || SKIP_DIRS.has(entry.name) || entry.name.startsWith('.')) continue;
		const skillDir = join(dir, entry.name);
		const location = join(skillDir, 'SKILL.md');
		if (!existsSync(location)) {
			if (depth + 1 < MAX_DEPTH) scanDir(root, scope, out, warnings, skillDir, depth + 1);
			continue;
		}
		const meta = parseFrontmatter(readFileSync(location, 'utf8'));
		if (!meta) {
			warnings.push(`${location}: frontmatter missing or unparseable, skipped`);
			continue;
		}
		const name = typeof meta.name === 'string' && meta.name.trim() ? meta.name.trim() : entry.name;
		const description = typeof meta.description === 'string' ? meta.description.trim() : '';
		if (!description) {
			warnings.push(`${location}: no description, skipped`);
			continue;
		}
		if (meta['disable-model-invocation'] === true) continue;
		if (name !== entry.name) warnings.push(`${location}: name "${name}" differs from folder name`);
		out.push({ name, description, location, scope });
	}
}

/** Profile skills override global ones with the same name. Sorted by name for a stable prompt. */
export function scanSkills(profileSkillsDir: string): { skills: Skill[]; warnings: string[] } {
	const warnings: string[] = [];
	const profileSkills: Skill[] = [];
	const globalSkills: Skill[] = [];
	scanDir(profileSkillsDir, 'profile', profileSkills, warnings);
	scanDir(paths.globalSkills, 'global', globalSkills, warnings);

	const byName = new Map<string, Skill>();
	for (const skill of globalSkills) {
		if (byName.has(skill.name)) warnings.push(`duplicate global skill "${skill.name}"`);
		else byName.set(skill.name, skill);
	}
	for (const skill of profileSkills) {
		if (byName.get(skill.name)?.scope === 'global') {
			warnings.push(`profile skill "${skill.name}" shadows the global one`);
		}
		byName.set(skill.name, skill);
	}
	const skills = [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
	return { skills, warnings };
}

function escapeXml(text: string): string {
	return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

export function renderSkillsCatalog(skills: Skill[]): string {
	const items = skills.map(
		(s) =>
			`  <skill>\n    <name>${escapeXml(s.name)}</name>\n    <description>${escapeXml(s.description)}</description>\n    <location>${escapeXml(s.location)}</location>\n  </skill>`
	);
	return `<available_skills>\n${items.join('\n')}\n</available_skills>`;
}

/** Creates <dir>/<name>/SKILL.md from a template and returns its path. */
export function createSkill(skillsDir: string, name: string, description: string): string {
	if (!isValidSkillName(name)) {
		throw new Error(
			`Invalid skill name "${name}": use lowercase letters, digits and single hyphens (max 64)`
		);
	}
	const dir = join(skillsDir, name);
	const location = join(dir, 'SKILL.md');
	if (existsSync(location)) throw new Error(`Skill already exists: ${location}`);
	mkdirSync(dir, { recursive: true });
	const title = name
		.split('-')
		.map((w) => w[0].toUpperCase() + w.slice(1))
		.join(' ');
	const desc = description.trim() || 'TODO: what this skill does and when to use it.';
	writeFileSync(
		location,
		`---
name: ${name}
description: ${JSON.stringify(desc)}
---

# ${title}

## When to use

TODO

## Steps

1. TODO

## Notes

- Things that went wrong before and how to avoid them.
`
	);
	return location;
}
