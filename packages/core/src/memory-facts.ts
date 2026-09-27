/**
 * The facts in memory files, and when each one was first seen. Pure logic: memory.ts reads and
 * writes the files and keeps the index next to them.
 */

/** Markdown to one line of plain text. */
function plain(markdown: string): string {
	return markdown
		.replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
		.replace(/(\*\*|__)(.+?)\1/g, '$2')
		.replace(/(?<!\w)([*_])(.+?)\1(?!\w)/g, '$2')
		.replace(/~~(.+?)~~/g, '$1')
		.replace(/`([^`]+)`/g, '$1')
		.replace(/\s+/g, ' ')
		.trim();
}

/**
 * The separate things a memory file says: list items, paragraphs, table rows and code blocks, as
 * plain text. Headings only group them. The agent is asked to write one fact per bullet, so this is
 * usually one entry per fact.
 */
export function parseFacts(text: string): string[] {
	return readFacts(text).map((fact) => fact.text);
}

/** A fact of a note, with where it is. */
export interface NoteFact {
	/** Plain text, as parseFacts has it. */
	text: string;
	/** The line it starts on, from 1. */
	line: number;
	/** The heading it is under, as plain text; a note's title (`# …`) doesn't count. */
	heading: string | null;
}

/** parseFacts, with each fact's line and heading. */
export function readFacts(text: string): NoteFact[] {
	const facts: NoteFact[] = [];
	let current: string[] = [];
	let start = 0;
	let heading: string | null = null;
	let fence = false;
	let lastWasRow = false;
	const flush = () => {
		if (current.length) facts.push({ text: current.join(' '), line: start, heading });
		current = [];
	};
	/** Adds a line to the fact being read, which starts here if it is the first. */
	const add = (part: string, at: number) => {
		if (!current.length) start = at;
		current.push(part);
	};
	for (const [i, raw] of text.split('\n').entries()) {
		const line = raw.trim();
		if (/^(```|~~~)/.test(line)) {
			fence = !fence;
			if (!fence) flush();
			continue;
		}
		if (fence) {
			if (line) add(line, i + 1);
			continue;
		}
		const isRow = line.startsWith('|');
		if (isRow && /^\|?[\s:|-]+$/.test(line)) {
			// A table's separator: the row above it was the header, not a fact.
			if (lastWasRow) facts.pop();
			continue;
		}
		lastWasRow = isRow;
		const title = line.match(/^(#{1,6})(\s|$)/);
		if (title) {
			flush();
			const words = line.slice(title[1].length).replace(/\s#+$/, '');
			heading = title[1].length > 1 ? plain(words) || null : null;
			continue;
		}
		if (!line || /^([-*_])(\s*\1){2,}$/.test(line)) {
			flush();
			continue;
		}
		if (isRow) {
			flush();
			facts.push({
				text: line
					.replace(/^\||\|$/g, '')
					.split('|')
					.map((cell) => cell.trim())
					.filter(Boolean)
					.join(' · '),
				line: i + 1,
				heading
			});
			continue;
		}
		const item = line.match(/^(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?(.*)$/);
		if (item) {
			flush();
			add(item[1], i + 1);
		} else {
			add(line.replace(/^>\s?/, ''), i + 1);
		}
	}
	flush();
	return facts.map((fact) => ({ ...fact, text: plain(fact.text) })).filter((fact) => fact.text);
}

/** Facts match by their words, so changing only case or spacing doesn't make one new. */
export function factKey(fact: string): string {
	return fact.toLowerCase().replace(/\s+/g, ' ').trim();
}

/**
 * When each fact was first seen, in ms; 0 means before dates were kept. Maps, not objects, so no
 * fact or file name can collide with Object.prototype.
 */
export interface FactIndex {
	/** Note path relative to the memory folder → fact key → date. */
	files: Map<string, Map<string, number>>;
	/** Facts that recently left a file, so one that moves to another file keeps its date. */
	removed: Map<string, number>;
}

const REMOVED_LIMIT = 500;

export function emptyFactIndex(): FactIndex {
	return { files: new Map(), removed: new Map() };
}

export function serializeFactIndex(index: FactIndex): string {
	return JSON.stringify({
		version: 1,
		files: [...index.files].map(([path, facts]) => [path, [...facts]]),
		removed: [...index.removed]
	});
}

/** Null if it isn't a fact index this version wrote. */
export function parseFactIndex(json: string): FactIndex | null {
	const isEntries = (value: unknown): value is [string, number][] =>
		Array.isArray(value) &&
		value.every((e) => Array.isArray(e) && typeof e[0] === 'string' && typeof e[1] === 'number');
	try {
		const data = JSON.parse(json) as { version?: unknown; files?: unknown; removed?: unknown };
		if (data.version !== 1 || !Array.isArray(data.files) || !isEntries(data.removed)) return null;
		const files = new Map<string, Map<string, number>>();
		for (const entry of data.files) {
			if (!Array.isArray(entry) || typeof entry[0] !== 'string' || !isEntries(entry[1]))
				return null;
			files.set(entry[0], new Map(entry[1]));
		}
		return { files, removed: new Map(data.removed) };
	} catch {
		return null;
	}
}

function forgetKey(index: FactIndex, key: string, date: number): void {
	// Re-inserting moves it to the end, so the oldest removals are dropped first.
	index.removed.delete(key);
	index.removed.set(key, date);
	for (const old of index.removed.keys()) {
		if (index.removed.size <= REMOVED_LIMIT) break;
		index.removed.delete(old);
	}
}

/** A date for a fact that is new to `path`: it may be in another file, or have just left one. */
function knownDate(index: FactIndex, key: string, path: string): number | undefined {
	for (const [other, facts] of index.files) {
		if (other !== path && facts.has(key)) return facts.get(key);
	}
	return index.removed.get(key);
}

/**
 * Dates the facts now in `path`: known ones keep their date, new ones get `at`. Returns whether
 * anything changed.
 */
export function noteFacts(index: FactIndex, path: string, text: string, at: number): boolean {
	const before = index.files.get(path);
	const after = new Map<string, number>();
	for (const fact of parseFacts(text)) {
		const key = factKey(fact);
		if (!after.has(key)) after.set(key, before?.get(key) ?? knownDate(index, key, path) ?? at);
	}
	let changed = !before || before.size !== after.size;
	for (const [key, date] of before ?? []) {
		if (after.has(key)) continue;
		forgetKey(index, key, date);
		changed = true;
	}
	for (const key of after.keys()) index.removed.delete(key);
	index.files.set(path, after);
	return changed;
}

function under(path: string, folder: string): boolean {
	return path === folder || path.startsWith(`${folder}/`);
}

/** A file or folder was deleted. */
export function forgetFacts(index: FactIndex, path: string): void {
	for (const [file, facts] of [...index.files]) {
		if (!under(file, path)) continue;
		for (const [key, date] of facts) forgetKey(index, key, date);
		index.files.delete(file);
	}
}

/** A file or folder was renamed. */
export function moveFacts(index: FactIndex, from: string, to: string): void {
	for (const [file, facts] of [...index.files]) {
		if (!under(file, from)) continue;
		index.files.delete(file);
		index.files.set(to + file.slice(from.length), facts);
	}
}
