import { randomUUID } from 'node:crypto';
import {
	existsSync,
	lstatSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	renameSync,
	rmSync,
	statSync,
	unlinkSync,
	writeFileSync
} from 'node:fs';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import {
	emptyFactIndex,
	factKey,
	forgetFacts,
	moveFacts,
	noteFacts,
	parseFactIndex,
	parseFacts,
	serializeFactIndex,
	type FactIndex
} from './memory-facts.ts';
import {
	aliasesOf,
	categoryOf,
	categoryProblem,
	isAliasLine,
	nameKey,
	titleIn
} from './memory-categories.ts';
import { profileDir, profileMemoryDir } from './paths.ts';

/*
 * A profile's long-term memory: short Markdown notes, one per topic, in its `memories` folder.
 * The agent reads and changes them with `nolune memory` (plain file commands work too); the family
 * sees them on the Memory page. `core.md` is pinned: every new chat starts with it in its prompt,
 * while the others are read when needed. Nothing here depends on the model provider.
 */

/** Notes are read into the context, so one stays small enough to read in one go. */
const MAX_NOTE_CHARS = 50_000;
/**
 * The pinned note, with what matters in almost every chat: it is copied whole into the system
 * prompt of every new conversation, instead of being read when needed.
 */
export const CORE_NOTE = 'core.md';
/** The pinned note costs its length in every chat, so it holds a few facts, not a topic's worth. */
export const MAX_PINNED_CHARS = 4_000;
const IMAGE = /\.(jpe?g|png|gif|webp|heic)$/i;
/** Where memory lived before: one file, pasted into each new system prompt. */
const LEGACY_FILE = 'MEMORY.md';
/** When each fact was first seen. Hidden, so listings skip it. */
const FACTS_FILE = '.facts.json';
/** How deep notes are looked for, so nothing the agent nests is hidden from the family. */
const MAX_DEPTH = 4;
/** Bigger files can only have been copied in by hand; they aren't notes. */
const MAX_BYTES = 1_000_000;

/** A refused change. The message says what to do instead. */
export class MemoryError extends Error {}

/** The note changed after the person opened it for editing. */
export class MemoryConflictError extends Error {}

function refuse(message: string): never {
	throw new MemoryError(message);
}

/** The profile's memory folder, created on first use. */
function openMemory(slug: string): string {
	const root = profileMemoryDir(slug);
	mkdirSync(root, { recursive: true, mode: 0o700 });
	importLegacyMemory(slug, root);
	return root;
}

/**
 * Conversations from before keep their frozen prompt, which tells the agent to keep everything in
 * <profile>/MEMORY.md. Whenever that file shows up it is moved into the folder, so nothing an
 * older chat saves gets lost.
 */
function importLegacyMemory(slug: string, root: string): void {
	const legacy = join(profileDir(slug), LEGACY_FILE);
	if (!existsSync(legacy)) return;
	if (!readFileSync(legacy, 'utf8').trim()) {
		unlinkSync(legacy);
		return;
	}
	let name = 'general.md';
	for (let i = 2; existsSync(join(root, name)); i++) name = `general-${i}.md`;
	renameSync(legacy, join(root, name));
}

/** A path relative to the folder, with forward slashes, like `people/anna.md`. */
function relPath(root: string, full: string): string {
	return relative(resolve(root), full).split(sep).join('/');
}

/**
 * A note's file, from a topic as people and the agent write it: `family`, `people/anna` or
 * `family.md`. Refuses anything outside the folder, hidden names and symbolic links.
 */
function notePath(root: string, topic: string): string {
	const clean = topic
		.trim()
		.replace(/\\/g, '/')
		.replace(/^\/+|\/+$/g, '');
	if (!clean || clean.includes('\0'))
		refuse('Which note? Give a topic like family or people/anna.');
	const file = /\.(md|markdown|txt)$/i.test(clean) ? clean : `${clean}.md`;
	const base = resolve(root);
	const full = resolve(base, file);
	if (!full.startsWith(base + sep)) refuse(`"${topic}" is outside the memory folder.`);
	if (
		relative(base, full)
			.split(sep)
			.some((part) => part.startsWith('.'))
	) {
		refuse(`"${topic}": names starting with a dot are reserved.`);
	}
	for (let current = full; current !== base; current = dirname(current)) {
		let isLink = false;
		try {
			isLink = lstatSync(current).isSymbolicLink();
		} catch {
			// Doesn't exist (yet).
		}
		if (isLink) refuse(`"${topic}" goes through a link, which memory doesn't follow.`);
	}
	return full;
}

/** A note's name as a title: `home.md` is "Home", and a person's `people/anna-smith.md` "Anna Smith". */
function titleOf(path: string): string {
	const words = basename(path)
		.replace(/\.[^.]+$/, '')
		.replace(/[-_]+/g, ' ')
		.trim();
	const upper = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);
	if (!words) return 'Notes';
	return categoryOf(path) === 'people' ? words.split(' ').map(upper).join(' ') : upper(words);
}

/** `path` is relative to the memory folder, like `core.md`. */
export function isPinnedNote(path: string): boolean {
	return path === CORE_NOTE;
}

/** The start of a text that grew past `max` some other way (an editor), cut at a line. */
export function cutAtLine(text: string, max: number): { text: string; cut: boolean } {
	if (text.length <= max) return { text, cut: false };
	const head = text.slice(0, max);
	const lastLine = head.lastIndexOf('\n');
	return { text: (lastLine > 0 ? head.slice(0, lastLine) : head).trimEnd(), cut: true };
}

function checkSize(text: string, path: string): void {
	if (isPinnedNote(path)) {
		if (text.length > MAX_PINNED_CHARS) {
			refuse(
				`${path} would be ${text.length} characters; a pinned note can have at most ${MAX_PINNED_CHARS}, because it goes into every chat. Keep only what matters in almost every conversation there, and move the rest to other notes.`
			);
		}
	} else if (text.length > MAX_NOTE_CHARS) {
		refuse(
			`${path} would be ${text.length} characters; a note can have at most ${MAX_NOTE_CHARS}. Split it into smaller notes by topic, or shorten it.`
		);
	}
}

/** Readers never see a half-written file. The temporary name is hidden from listings. */
function writeAtomic(full: string, text: string): void {
	mkdirSync(dirname(full), { recursive: true, mode: 0o700 });
	const temp = join(dirname(full), `.tmp-${randomUUID()}`);
	try {
		writeFileSync(temp, text, { mode: 0o600 });
		renameSync(temp, full);
	} finally {
		rmSync(temp, { force: true });
	}
}

function readNote(root: string, full: string, topic: string): string {
	if (!existsSync(full) || !statSync(full).isFile()) {
		refuse(`There is no note "${topic}". \`nolune memory\` lists them.`);
	}
	return readFileSync(full, 'utf8');
}

// --- Fact dates ---

interface StoredFile {
	path: string;
	text: string;
	size: number;
	updatedAt: number;
}

/** Every note, sorted by path. */
function readMemoryFiles(root: string): StoredFile[] {
	const files: StoredFile[] = [];
	const walk = (dir: string, depth: number) => {
		for (const name of readdirSync(dir).sort()) {
			if (name.startsWith('.') || name === 'node_modules') continue;
			const full = join(dir, name);
			let stat;
			try {
				stat = lstatSync(full);
			} catch {
				continue;
			}
			if (stat.isDirectory()) {
				if (depth < MAX_DEPTH) walk(full, depth + 1);
			} else if (stat.isFile() && !IMAGE.test(name) && stat.size <= MAX_BYTES) {
				files.push({
					path: relPath(root, full),
					text: readFileSync(full, 'utf8'),
					size: stat.size,
					updatedAt: stat.mtimeMs
				});
			}
		}
	};
	walk(root, 1);
	return files;
}

/**
 * The fact dates, caught up with the files on disk: facts that got there some other way (a
 * command, an editor) are dated by their file's modification time. The first time, everything
 * already in memory is dated 0: before dates were kept. Null if it can't be read, because
 * this bookkeeping must never stop memory itself from working.
 */
function loadFacts(
	root: string,
	files = readMemoryFiles(root)
): { index: FactIndex; changed: boolean } | null {
	try {
		const file = join(root, FACTS_FILE);
		const stored = existsSync(file) ? parseFactIndex(readFileSync(file, 'utf8')) : null;
		const index = stored ?? emptyFactIndex();
		let changed = !stored;
		for (const f of files) {
			if (noteFacts(index, f.path, f.text, stored ? Math.floor(f.updatedAt) : 0)) changed = true;
		}
		const present = new Set(files.map((f) => f.path));
		for (const path of index.files.keys()) {
			if (present.has(path)) continue;
			forgetFacts(index, path);
			changed = true;
		}
		return { index, changed };
	} catch (err) {
		console.error('[nolune] could not read memory fact dates:', err);
		return null;
	}
}

function saveFacts(root: string, index: FactIndex): void {
	try {
		writeAtomic(join(root, FACTS_FILE), serializeFactIndex(index));
	} catch (err) {
		console.error('[nolune] could not save memory fact dates:', err);
	}
}

/**
 * Makes a change to the notes and dates it. The dates are caught up first, so a change is never
 * mistaken for something that happened before.
 */
function changing<T>(
	root: string,
	change: () => T,
	record: (index: FactIndex, now: number) => void
): T {
	const facts = loadFacts(root);
	const result = change();
	if (facts) {
		record(facts.index, Date.now());
		saveFacts(root, facts.index);
	}
	return result;
}

/** Writes a note and dates its new facts. */
function saveNote(root: string, full: string, text: string): void {
	checkSize(text, relPath(root, full));
	changing(
		root,
		() => writeAtomic(full, text),
		(index, now) => noteFacts(index, relPath(root, full), text, now)
	);
}

function lineNumbers(text: string, match: (line: string) => boolean): number[] {
	return text.split('\n').flatMap((line, i) => (match(line) ? [i + 1] : []));
}

// --- Reading ---

export interface MemoryFact {
	/** Plain text. */
	text: string;
	/** When nolune first saw it, in ms. Null: before dates were kept. */
	learnedAt: number | null;
}

export interface MemoryFile {
	/** Relative to the memory folder, like `family.md` or `people/anna.md`. */
	path: string;
	text: string;
	facts: MemoryFact[];
	size: number;
	/** Modification time in ms. Also the version an edit is based on. */
	updatedAt: number;
}

/** Every note in the profile's memory, sorted by path, with its facts and their dates. */
export function listMemoryFiles(slug: string): MemoryFile[] {
	const root = openMemory(slug);
	const files = readMemoryFiles(root);
	const facts = loadFacts(root, files);
	if (facts?.changed) saveFacts(root, facts.index);
	return files.map((file) => {
		const dates = facts?.index.files.get(file.path);
		return {
			...file,
			facts: parseFacts(file.text).map((text) => ({
				text,
				learnedAt: dates?.get(factKey(text)) || null
			}))
		};
	});
}

/** The notes' paths, without reading the fact dates: for the system prompt. */
export function listMemoryNotes(slug: string): string[] {
	return readMemoryFiles(openMemory(slug)).map((file) => file.path);
}

/** Every note with its text, sorted by path, without the fact dates: for the new-chat suggestions. */
export function readMemoryNotes(slug: string): { path: string; text: string; updatedAt: number }[] {
	return readMemoryFiles(openMemory(slug)).map(({ path, text, updatedAt }) => ({
		path,
		text,
		updatedAt
	}));
}

/** The pinned note as the system prompt shows it (see cutAtLine); null while empty or missing. */
export function readPinnedNote(slug: string, path: string): { text: string; cut: boolean } | null {
	const root = openMemory(slug);
	let text: string;
	try {
		const full = notePath(root, path);
		const stat = lstatSync(full);
		// Like the listing: no links, nothing only a person could have copied in.
		if (!stat.isFile() || stat.size > MAX_BYTES) return null;
		text = readFileSync(full, 'utf8').trim();
	} catch {
		return null;
	}
	return text ? cutAtLine(text, MAX_PINNED_CHARS) : null;
}

export function readMemoryNote(slug: string, topic: string): { path: string; text: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	return { path: relPath(root, full), text: readNote(root, full, topic) };
}

// --- Changes, for `nolune memory` ---

/**
 * The facts in `text`, one per line, without their bullets: a list given as one fact (by a model,
 * or a command's quoted argument) would otherwise be joined into one line.
 */
export function factLines(text: string): string[] {
	return text
		.split(/\r?\n/)
		.map((line) =>
			line
				.replace(/\s+/g, ' ')
				.trim()
				.replace(/^[-*+]\s+/, '')
		)
		.filter(Boolean);
}

/**
 * Adds one fact as a bullet at the end of a note, or of the part under the heading `under`,
 * creating the note if needed. A person's "Also called" that names no one new is a duplicate.
 */
export function addMemoryFact(
	slug: string,
	topic: string,
	fact: string,
	under?: string
): { path: string; created: boolean; duplicate: boolean; line: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	const path = relPath(root, full);
	const line = fact
		.replace(/\s+/g, ' ')
		.trim()
		.replace(/^[-*+]\s+/, '');
	if (!line) refuse('The fact is empty.');
	const exists = existsSync(full);
	if (exists && !statSync(full).isFile()) refuse(`"${topic}" is a folder, not a note.`);
	if (!categoryOf(path)) refuse(categoryProblem(path, exists));
	const before = exists ? readFileSync(full, 'utf8') : '';
	const key = factKey(parseFacts(`- ${line}`)[0] ?? line);
	if (
		parseFacts(before).some((known) => factKey(known) === key) ||
		namesNoOneNew(path, before, line)
	) {
		return { path, created: false, duplicate: true, line: `- ${line}` };
	}
	const lines = (before.trim() ? before.trimEnd() : `# ${titleOf(path)}`).split('\n');
	saveNote(root, full, `${withFact(lines, `- ${line}`, under).join('\n')}\n`);
	return { path, created: !exists, duplicate: false, line: `- ${line}` };
}

/** A person's "Also called" line whose names their note has already, its title or its others. */
function namesNoOneNew(path: string, note: string, line: string): boolean {
	if (categoryOf(path) !== 'people' || !isAliasLine(line)) return false;
	const known = new Set(
		[titleOf(path), titleIn(note), ...aliasesOf(note)].flatMap((name) =>
			name ? [nameKey(name)] : []
		)
	);
	return aliasesOf(line).every((name) => known.has(nameKey(name)));
}

/**
 * Adds facts learned elsewhere (an import) to a note, under `## heading` when given, keeping the
 * dates they were first seen there: a fact without one is dated like those from before dates
 * were kept. Facts the note already has are skipped. A pinned note takes only what fits; the
 * rest comes back in `left`.
 */
export function addMemoryFacts(
	slug: string,
	topic: string,
	facts: { text: string; learnedAt: number | null }[],
	heading?: string
): { path: string; added: MemoryFact[]; left: MemoryFact[] } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	const path = relPath(root, full);
	if (existsSync(full) && !statSync(full).isFile()) refuse(`"${topic}" is a folder, not a note.`);
	if (!categoryOf(path)) refuse(categoryProblem(path, existsSync(full)));
	const before = existsSync(full) ? readFileSync(full, 'utf8') : '';
	const known = new Set(parseFacts(before).map(factKey));
	let lines = (before.trim() ? before.trimEnd() : `# ${titleOf(path)}`).split('\n');
	const added: MemoryFact[] = [];
	const left: MemoryFact[] = [];
	for (const fact of facts) {
		const line = fact.text
			.replace(/\s+/g, ' ')
			.trim()
			.replace(/^[-*+]\s+/, '');
		const key = factKey(parseFacts(`- ${line}`)[0] ?? line);
		if (!line || known.has(key)) continue;
		const next = withFact(lines, `- ${line}`, heading);
		if (isPinnedNote(path) && next.join('\n').length + 1 > MAX_PINNED_CHARS) {
			left.push({ text: line, learnedAt: fact.learnedAt });
			continue;
		}
		lines = next;
		known.add(key);
		added.push({ text: line, learnedAt: fact.learnedAt });
	}
	if (!added.length) return { path, added, left };
	const text = `${lines.join('\n')}\n`;
	checkSize(text, path);
	changing(
		root,
		() => writeAtomic(full, text),
		(index, now) => {
			noteFacts(index, path, text, now);
			const dates = index.files.get(path);
			for (const fact of added) {
				const key = factKey(parseFacts(`- ${fact.text}`)[0] ?? fact.text);
				// Only facts that are new to memory; one it already had elsewhere keeps its date.
				if (dates?.get(key) === now) dates.set(key, fact.learnedAt ?? 0);
			}
		}
	);
	return { path, added, left };
}

/**
 * The note's lines with a bullet added at the end of the part under `heading`, a heading of any
 * level (the note's title too) up to the next one, as readFacts tells which heading a fact is
 * under; it's started as `## heading` at the end when the note lacks it. Without one, at the end.
 */
function withFact(lines: string[], bullet: string, heading?: string): string[] {
	const out = [...lines];
	const title = heading
		?.replace(/^#+\s*/, '')
		.replace(/\s+/g, ' ')
		.trim();
	if (!title) return [...out, ...(out.at(-1)?.startsWith('- ') ? [] : ['']), bullet];
	const headingOf = (line: string) => line.trim().match(/^(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/);
	const start = out.findIndex((l) => headingOf(l)?.[2].toLowerCase() === title.toLowerCase());
	if (start === -1) return [...out, '', `## ${title}`, '', bullet];
	let end = out.findIndex((l, i) => i > start && headingOf(l));
	if (end === -1) end = out.length;
	// After the part's last line, before the blank lines leading to the next heading.
	while (end > start + 1 && !out[end - 1].trim()) end--;
	out.splice(end, 0, ...(end === start + 1 ? [''] : []), bullet);
	return out;
}

/** Replaces text that appears exactly once in a note. */
/**
 * Replaces the one place `oldText` is in a note. Returns the whole lines it touched, as they were
 * (`before`) and as they are now (`after`), for putting them back (revertMemoryLines).
 */
export function replaceInMemory(
	slug: string,
	topic: string,
	oldText: string,
	newText: string
): { path: string; before: string; after: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	const path = relPath(root, full);
	const text = readNote(root, full, topic);
	if (!oldText) refuse('Give the text to replace.');
	const hits: number[] = [];
	for (let at = text.indexOf(oldText); at !== -1; at = text.indexOf(oldText, at + 1)) hits.push(at);
	if (hits.length === 0) {
		refuse(`"${oldText}" isn't in ${path}. See it with \`nolune memory show ${topic}\`.`);
	}
	if (hits.length > 1) {
		const lines = hits.map((at) => text.slice(0, at).split('\n').length);
		refuse(
			`"${oldText}" is in ${path} ${hits.length} times (lines ${lines.join(', ')}). Include more of the text so it matches once.`
		);
	}
	const at = hits[0];
	// Sliced, not String.replace: `$&` and friends in the new text are meant literally.
	const changed = text.slice(0, at) + newText + text.slice(at + oldText.length);
	saveNote(root, full, changed);
	const start = text.lastIndexOf('\n', at - 1) + 1;
	const next = text.indexOf('\n', at + oldText.length);
	const end = next === -1 ? text.length : next;
	return {
		path,
		before: text.slice(start, end),
		after: changed.slice(start, end + newText.length - oldText.length)
	};
}

/**
 * Puts back what a change left in a note: the whole lines `current`, where they are once, become
 * `restore`, or go when it's null. A note left without facts goes too when `dropEmpty`. Refuses
 * when the lines aren't there once any more, since someone changed them since.
 */
export function revertMemoryLines(
	slug: string,
	topic: string,
	current: string,
	restore: string | null,
	dropEmpty = false
): { path: string; removedNote: boolean } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	const path = relPath(root, full);
	const lines = readNote(root, full, topic).split('\n');
	const wanted = current.split('\n');
	const starts: number[] = [];
	for (let i = 0; i + wanted.length <= lines.length; i++) {
		if (wanted.every((line, j) => lines[i + j] === line)) starts.push(i);
	}
	if (starts.length !== 1) refuse(`${path} changed since, so that can't be put back.`);
	lines.splice(starts[0], wanted.length, ...(restore === null ? [] : restore.split('\n')));
	const text = lines.join('\n');
	if (dropEmpty && !parseFacts(text).length) {
		changing(
			root,
			() => unlinkSync(full),
			(index) => forgetFacts(index, path)
		);
		return { path, removedNote: true };
	}
	saveNote(root, full, text);
	return { path, removedNote: false };
}

/** Removes the one line of a note that contains `match` (ignoring case). */
export function forgetMemoryFact(
	slug: string,
	topic: string,
	match: string
): { path: string; removed: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	const path = relPath(root, full);
	const text = readNote(root, full, topic);
	const needle = match.trim().toLowerCase();
	if (!needle) refuse('Give some text from the fact to forget.');
	const lines = text.split('\n');
	const hits = lineNumbers(
		text,
		(line) => !/^\s*#/.test(line) && line.toLowerCase().includes(needle)
	);
	if (hits.length === 0) {
		refuse(`Nothing in ${path} contains "${match}". See it with \`nolune memory show ${topic}\`.`);
	}
	if (hits.length > 1) {
		refuse(
			`Several lines of ${path} contain "${match}"; give more of the text:\n${hits.map((n) => `  ${n}: ${lines[n - 1].trim()}`).join('\n')}`
		);
	}
	const [removed] = lines.splice(hits[0] - 1, 1);
	saveNote(root, full, lines.join('\n'));
	return { path, removed: removed.trim() };
}

/** Replaces a whole note, e.g. to reorganize it. */
export function writeMemoryNote(
	slug: string,
	topic: string,
	text: string
): { path: string; created: boolean } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	if (!text.trim()) refuse('The note is empty. To delete it, use `nolune memory rm`.');
	if (existsSync(full) && !statSync(full).isFile()) refuse(`"${topic}" is a folder, not a note.`);
	const created = !existsSync(full);
	// A note from before the categories can be rewritten; a new one goes into one.
	if (created && !categoryOf(relPath(root, full)))
		refuse(categoryProblem(relPath(root, full), false));
	saveNote(root, full, text.endsWith('\n') ? text : `${text}\n`);
	return { path: relPath(root, full), created };
}

export function removeMemoryNote(slug: string, topic: string): { path: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	readNote(root, full, topic);
	const path = relPath(root, full);
	changing(
		root,
		() => unlinkSync(full),
		(index) => forgetFacts(index, path)
	);
	return { path };
}

export function renameMemoryNote(
	slug: string,
	from: string,
	to: string
): { from: string; to: string } {
	const root = openMemory(slug);
	const source = notePath(root, from);
	const target = notePath(root, to);
	const text = readNote(root, source, from);
	if (existsSync(target)) refuse(`There already is a note "${to}".`);
	const moved = { from: relPath(root, source), to: relPath(root, target) };
	if (!categoryOf(moved.to)) refuse(categoryProblem(moved.to, false));
	// Into people/ from elsewhere, it's titled with their name, like every person's note.
	const retitled =
		categoryOf(moved.to) === 'people' && categoryOf(moved.from) !== 'people'
			? withTitle(text, titleOf(moved.to))
			: null;
	checkSize(retitled ?? text, moved.to);
	changing(
		root,
		() => {
			mkdirSync(dirname(target), { recursive: true, mode: 0o700 });
			renameSync(source, target);
			if (retitled !== null) writeAtomic(target, retitled);
		},
		(index) => moveFacts(index, moved.from, moved.to)
	);
	return moved;
}

/** The note with `title` as its `# title`, in place of the one it had. */
function withTitle(text: string, title: string): string {
	const lines = text.split('\n');
	const first = lines.findIndex((line) => line.trim());
	if (first !== -1 && /^#\s/.test(lines[first].trim())) lines[first] = `# ${title}`;
	else lines.unshift(`# ${title}`, '');
	return lines.join('\n');
}

/**
 * A note's body in the pieces a merge moves, each under the heading it was under (null: the
 * note's title, or none): a bullet with its indented lines, a paragraph, a table, a code block.
 */
function noteParts(text: string): { title: string | null; parts: [string | null, string][] } {
	const parts: [string | null, string][] = [];
	let title: string | null = null;
	let heading: string | null = null;
	let piece: string[] = [];
	let kind: 'bullet' | 'text' | 'table' | 'code' | null = null;
	const flush = () => {
		if (piece.length) parts.push([heading, piece.join('\n')]);
		piece = [];
		kind = null;
	};
	for (const raw of text.split('\n')) {
		const line = raw.trimEnd();
		if (kind === 'code') {
			piece.push(line);
			if (/^\s*(```|~~~)/.test(line)) flush();
			continue;
		}
		const head = line.match(/^(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/);
		if (head) {
			flush();
			if (head[1] === '#' && title === null && !parts.length && heading === null) title = head[2];
			else heading = head[2];
			continue;
		}
		if (!line.trim()) {
			if (kind !== 'bullet') flush();
			continue;
		}
		if (/^\s*(```|~~~)/.test(line)) {
			flush();
			kind = 'code';
			piece.push(line);
		} else if (/^[-*+]\s+/.test(line)) {
			flush();
			kind = 'bullet';
			piece.push(line);
		} else if (kind === 'bullet' && /^\s/.test(line)) {
			piece.push(line);
		} else {
			const next = line.startsWith('|') ? 'table' : 'text';
			if (kind !== next) flush();
			kind = next;
			piece.push(line);
		}
	}
	flush();
	return { title, parts };
}

/**
 * Puts what one note says into another about the same thing (two notes about one person, say)
 * and deletes it: each piece under the heading it was under, with the dates its facts were
 * learned; what the other note says already is left out. Into a person's note, the other's title
 * and what it was called go into its "Also called" line. Into a note that isn't there yet, it's a
 * move (`merged` is false).
 */
export function mergeMemoryNotes(
	slug: string,
	from: string,
	into: string
): { from: string; into: string; added: number; merged: boolean } {
	const root = openMemory(slug);
	const source = notePath(root, from);
	const target = notePath(root, into);
	const fromText = readNote(root, source, from);
	const merged = { from: relPath(root, source), into: relPath(root, target) };
	if (merged.from === merged.into) refuse('That is the same note.');
	if (isPinnedNote(merged.from) || isPinnedNote(merged.into)) {
		refuse('core is pinned and kept small: move facts in or out of it one at a time.');
	}
	if (existsSync(target)) {
		// The same file by another name, on a disk that ignores case: merging would delete it.
		const [a, b] = [statSync(source), statSync(target)];
		if (a.ino === b.ino && a.dev === b.dev) refuse('That is the same note.');
	} else {
		const moved = renameMemoryNote(slug, from, into);
		return { from: moved.from, into: moved.to, added: parseFacts(fromText).length, merged: false };
	}
	const intoText = readNote(root, target, into);
	if (!categoryOf(merged.into)) refuse(categoryProblem(merged.into, true));

	const person = categoryOf(merged.into) === 'people';
	const known = new Set(parseFacts(intoText).map(factKey));
	const { title: fromTitle, parts } = noteParts(fromText);
	const intoTitle = titleIn(intoText);
	let lines = intoText.trimEnd().split('\n');
	let added = 0;
	for (const [heading, piece] of parts) {
		if (person && isAliasLine(piece)) continue;
		const keys = parseFacts(piece).map(factKey);
		if (!keys.length || keys.every((key) => known.has(key))) continue;
		lines = withFact(lines, piece, heading ?? intoTitle ?? undefined);
		for (const key of keys) known.add(key);
		added++;
	}
	if (person) {
		const taken = new Set(
			[intoTitle, ...aliasesOf(intoText)].filter(Boolean).map((n) => nameKey(n!))
		);
		const names = [fromTitle, ...aliasesOf(fromText)].filter((name): name is string => {
			const key = name ? nameKey(name) : '';
			if (!key || taken.has(key)) return false;
			taken.add(key);
			return true;
		});
		const at = lines.findIndex(isAliasLine);
		if (names.length && at !== -1) lines[at] = `${lines[at].trimEnd()}, ${names.join(', ')}`;
		else if (names.length)
			lines = withFact(lines, `- Also called: ${names.join(', ')}`, intoTitle ?? undefined);
	}
	const text = `${lines.join('\n')}\n`;
	checkSize(text, merged.into);
	changing(
		root,
		() => {
			writeAtomic(target, text);
			unlinkSync(source);
		},
		(index, now) => {
			const learned = index.files.get(merged.from);
			noteFacts(index, merged.into, text, now);
			const dates = index.files.get(merged.into);
			for (const [key, at] of learned ?? []) if (dates?.get(key) === now) dates.set(key, at);
			forgetFacts(index, merged.from);
		}
	);
	return { ...merged, added, merged: true };
}

// --- The Memory page ---

/**
 * Saves a note someone edited on the Memory page. `basedOn` is the `updatedAt` they started from,
 * or 0 for a note that didn't exist yet (the page offers to start the core note); if the agent
 * changed the note since, nothing is written.
 */
export function writeMemoryFile(slug: string, path: string, text: string, basedOn: number): void {
	const root = openMemory(slug);
	const full = notePath(root, path);
	if (!existsSync(full)) {
		if (basedOn) throw new MemoryConflictError(`${path} was deleted while you were editing it.`);
	} else if (!statSync(full).isFile()) {
		refuse(`"${path}" is a folder, not a note.`);
	} else if (statSync(full).mtimeMs !== basedOn) {
		throw new MemoryConflictError(
			basedOn
				? `nolune changed ${path} while you were editing it.`
				: `nolune started ${path} while you were writing it.`
		);
	}
	saveNote(root, full, text);
}

export function forgetMemoryFile(slug: string, path: string): void {
	const root = openMemory(slug);
	const full = notePath(root, path);
	if (!existsSync(full) || !statSync(full).isFile()) return;
	changing(
		root,
		() => unlinkSync(full),
		(index) => forgetFacts(index, relPath(root, full))
	);
}
