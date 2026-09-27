/**
 * What another assistant (ChatGPT, Claude, Gemini) remembers about someone, as it writes it out
 * when asked with EXPORT_PROMPT. Pure, so the welcome page can count sections while people paste,
 * and the gateway parses the same text again before anything is saved (memory-import.ts).
 */

/** The sections the prompt asks for, in its order. `other` is anything under another heading. */
export const EXPORT_SECTIONS = [
	'instructions',
	'identity',
	'career',
	'projects',
	'preferences',
	'other'
] as const;
export type ExportSection = (typeof EXPORT_SECTIONS)[number];

export interface ExportedFact {
	section: ExportSection;
	text: string;
	/** `YYYY-MM-DD`, or null for `[unknown]` and undated lines. */
	date: string | null;
}

/**
 * What people paste into their old assistant. In English whatever the interface's language, like
 * image templates: it's a prompt, and the headings it asks for are what the parser looks for.
 */
export const EXPORT_PROMPT = `Export all of my stored memories and any context you've learned about me from past conversations. Preserve my words verbatim where possible, especially for instructions and preferences.

## Categories (output in this order):

1. **Instructions**: Rules I've explicitly asked you to follow going forward — tone, format, style, "always do X", "never do Y", and corrections to your behavior. Only include rules from stored memories, not from conversations.

2. **Identity**: Name, age, location, education, family, relationships, languages, and personal interests.

3. **Career**: Current and past roles, companies, and general skill areas.

4. **Projects**: Projects I meaningfully built or committed to. Ideally ONE entry per project. Include what it does, current status, and any key decisions. Use the project name or a short descriptor as the first words of the entry.

5. **Preferences**: Opinions, tastes, and working-style preferences that apply broadly.

## Format:

Use section headers for each category. Within each category, list one entry per line, sorted by oldest date first. Format each line as:

[YYYY-MM-DD] - Entry content here.

If no date is known, use [unknown] instead.

## Output:
- Wrap the entire export in a single code block for easy copying.
- After the code block, state whether this is the complete set or if more remain.`;

/** More than this is not one person's memory; the rest is left out. */
export const MAX_EXPORTED_FACTS = 500;

const SECTION_WORDS: [Exclude<ExportSection, 'other'>, RegExp][] = [
	['instructions', /\binstructions?\b|\brules?\b/i],
	['identity', /\bidentity\b|\babout me\b/i],
	['career', /\bcareer\b|\bwork\b|\bjobs?\b/i],
	['projects', /\bprojects?\b/i],
	['preferences', /\bpreferences?\b|\btastes?\b/i]
];

/** `## Career`, `**Career**`, `2. **Career**:` or `Career:` alone on its line. */
function headingOf(line: string): string | null {
	const hashes = line.match(/^#{1,6}\s+(.+?)\s*#*$/);
	if (hashes) return hashes[1];
	const bold = line.match(/^(?:\d+[.)]\s*)?(\*\*|__)(.+?)\1\s*:?$/);
	if (bold) return bold[2];
	const label = line.match(/^(?:\d+[.)]\s*)?([\p{L} &/]{3,40}):$/u);
	return label ? label[1] : null;
}

function sectionOf(heading: string): ExportSection {
	const plain = heading.replace(/[*_`]/g, '');
	return SECTION_WORDS.find(([, words]) => words.test(plain))?.[0] ?? 'other';
}

/** A real day that has already happened, or null. */
function dayOf(value: string): string | null {
	const match = value.match(/^(\d{4})-(\d{2})-(\d{2})$/);
	if (!match) return null;
	const at = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
	const date = new Date(at);
	if (date.getUTCDate() !== Number(match[3]) || date.getUTCFullYear() < 1990) return null;
	return at <= Date.now() + 24 * 60 * 60 * 1000 ? value : null;
}

/**
 * The code blocks' contents when the reply has any: what the assistant says around them ("This is
 * the complete set") isn't memory. Otherwise the whole text, as when only the block was copied.
 */
function exportBody(text: string): { body: string; fenced: boolean } {
	const blocks = [...text.matchAll(/^[ \t]*(```|~~~)[^\n]*\n([\s\S]*?)^[ \t]*\1[ \t]*$/gm)];
	if (!blocks.length) return { body: text, fenced: false };
	return { body: blocks.map((b) => b[2]).join('\n'), fenced: true };
}

/**
 * The facts in an export, in order. Lines are `[2025-03-02] - text` or `[unknown] - text` under a
 * heading that names the section. Inside a code block any line under a heading counts; without
 * one, only dated lines and list items do, so the assistant's own sentences are left out.
 */
export function parseMemoryExport(text: string): ExportedFact[] {
	const { body, fenced } = exportBody(text.replace(/\r\n?/g, '\n'));
	const facts: ExportedFact[] = [];
	let section: ExportSection | null = null;
	for (const raw of body.split('\n')) {
		const line = raw.trim();
		if (!line) continue;
		const heading = headingOf(line);
		if (heading !== null) {
			section = sectionOf(heading);
			continue;
		}
		if (!section) continue;
		const item = line.match(/^(?:[-*+•]|\d+[.)])\s+(.*)$/);
		const content = (item ? item[1] : line).trim();
		const dated = content.match(/^\[([^\]]{1,20})\]\s*(?:[-–—:]\s*)?(.*)$/);
		if (!dated && !item && !fenced) continue;
		const entry = (dated ? dated[2] : content).trim();
		if (!entry) continue;
		facts.push({ section, text: entry, date: dated ? dayOf(dated[1].trim()) : null });
		if (facts.length >= MAX_EXPORTED_FACTS) break;
	}
	return facts;
}

/** How many facts each section has, for the chips under the box. */
export function countSections(facts: ExportedFact[]): Record<ExportSection, number> {
	const counts = Object.fromEntries(EXPORT_SECTIONS.map((s) => [s, 0])) as Record<
		ExportSection,
		number
	>;
	for (const fact of facts) counts[fact.section]++;
	return counts;
}

/**
 * The project an entry is about, from its first words: `Tidepool: a Godot farming game` is
 * Tidepool. Null when it doesn't start with a short name, and the entry goes in a shared note.
 */
export function projectName(text: string): string | null {
	const match = text.match(/^(.{1,48}?)(?::\s|\s[-–—]\s|\s\()/);
	if (!match) return null;
	const name = match[1].replace(/[*_`"“”]/g, '').trim();
	return name && name.split(/\s+/).length <= 6 ? name : null;
}
