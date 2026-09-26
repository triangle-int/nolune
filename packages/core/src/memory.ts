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
import { profileDir, profileMemoryDir } from './paths.ts';

/*
 * A profile's long-term memory: short Markdown notes, one per topic, in its `memories` folder.
 * The agent reads and changes them with `btw memory` (plain file commands work too); the family
 * sees them on the Memory page. Nothing here depends on the model provider.
 */

/** Notes are read into the context, so one stays small enough to read in one go. */
const MAX_NOTE_CHARS = 50_000;
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

/** A note's name as a title: `people/anna-smith.md` is "Anna smith". */
function titleOf(path: string): string {
	const words = basename(path)
		.replace(/\.[^.]+$/, '')
		.replace(/[-_]+/g, ' ')
		.trim();
	return words ? words.charAt(0).toUpperCase() + words.slice(1) : 'Notes';
}

function checkSize(text: string, path: string): void {
	if (text.length > MAX_NOTE_CHARS) {
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
		refuse(`There is no note "${topic}". \`btw memory\` lists them.`);
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
		console.error('[btw] could not read memory fact dates:', err);
		return null;
	}
}

function saveFacts(root: string, index: FactIndex): void {
	try {
		writeAtomic(join(root, FACTS_FILE), serializeFactIndex(index));
	} catch (err) {
		console.error('[btw] could not save memory fact dates:', err);
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
	/** When btw first saw it, in ms. Null: before dates were kept. */
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

export function readMemoryNote(slug: string, topic: string): { path: string; text: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	return { path: relPath(root, full), text: readNote(root, full, topic) };
}

// --- Changes, for `btw memory` ---

/** Adds one fact as a bullet at the end of a note, creating the note if needed. */
export function addMemoryFact(
	slug: string,
	topic: string,
	fact: string
): { path: string; created: boolean; duplicate: boolean } {
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
	const before = exists ? readFileSync(full, 'utf8') : '';
	const key = factKey(parseFacts(`- ${line}`)[0] ?? line);
	if (parseFacts(before).some((known) => factKey(known) === key)) {
		return { path, created: false, duplicate: true };
	}
	const text = before.trim()
		? `${before.trimEnd()}\n- ${line}\n`
		: `# ${titleOf(path)}\n\n- ${line}\n`;
	saveNote(root, full, text);
	return { path, created: !exists, duplicate: false };
}

/** Replaces text that appears exactly once in a note. */
export function replaceInMemory(
	slug: string,
	topic: string,
	oldText: string,
	newText: string
): { path: string } {
	const root = openMemory(slug);
	const full = notePath(root, topic);
	const path = relPath(root, full);
	const text = readNote(root, full, topic);
	if (!oldText) refuse('Give the text to replace.');
	const hits: number[] = [];
	for (let at = text.indexOf(oldText); at !== -1; at = text.indexOf(oldText, at + 1)) hits.push(at);
	if (hits.length === 0) {
		refuse(`"${oldText}" isn't in ${path}. See it with \`btw memory show ${topic}\`.`);
	}
	if (hits.length > 1) {
		const lines = hits.map((at) => text.slice(0, at).split('\n').length);
		refuse(
			`"${oldText}" is in ${path} ${hits.length} times (lines ${lines.join(', ')}). Include more of the text so it matches once.`
		);
	}
	// Sliced, not String.replace: `$&` and friends in the new text are meant literally.
	saveNote(root, full, text.slice(0, hits[0]) + newText + text.slice(hits[0] + oldText.length));
	return { path };
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
		refuse(`Nothing in ${path} contains "${match}". See it with \`btw memory show ${topic}\`.`);
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
	if (!text.trim()) refuse('The note is empty. To delete it, use `btw memory rm`.');
	if (existsSync(full) && !statSync(full).isFile()) refuse(`"${topic}" is a folder, not a note.`);
	const created = !existsSync(full);
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
	readNote(root, source, from);
	if (existsSync(target)) refuse(`There already is a note "${to}".`);
	const moved = { from: relPath(root, source), to: relPath(root, target) };
	changing(
		root,
		() => {
			mkdirSync(dirname(target), { recursive: true, mode: 0o700 });
			renameSync(source, target);
		},
		(index) => moveFacts(index, moved.from, moved.to)
	);
	return moved;
}

// --- The Memory page ---

/**
 * Saves a note someone edited on the Memory page. `basedOn` is the `updatedAt` they started from;
 * if the agent changed the note since, nothing is written.
 */
export function writeMemoryFile(slug: string, path: string, text: string, basedOn: number): void {
	const root = openMemory(slug);
	const full = notePath(root, path);
	if (!existsSync(full) || !statSync(full).isFile()) {
		throw new MemoryConflictError(`${path} was deleted while you were editing it.`);
	}
	if (statSync(full).mtimeMs !== basedOn) {
		throw new MemoryConflictError(`btw changed ${path} while you were editing it.`);
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
