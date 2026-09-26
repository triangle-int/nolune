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
import { dirname, join, relative, resolve, sep } from 'node:path';
import type Anthropic from '@anthropic-ai/sdk';
import { profileDir, profileMemoryDir } from './paths.ts';

/**
 * Anthropic's memory tool. The model asks for file operations under /memories and they run on the
 * profile's `memories` folder. The API adds its own instructions for it to the system prompt.
 * Constant, like run_command: it is part of the cached prefix.
 */
export const MEMORY_TOOL: Anthropic.MemoryTool20250818 = {
	type: 'memory_20250818',
	name: 'memory'
};

const ROOT = '/memories';
/** Memory is read into the context, so a file stays small enough to read in one go. */
const MAX_FILE_CHARS = 50_000;
/** The tool's description tells the model that longer files are cut and read with view_range. */
const VIEW_CHARS = 16_000;
const MAX_LINES = 999_999;
const IMAGE = /\.(jpe?g|png|gif|webp)$/i;
/** Where memory lived before the memory tool: one file, pasted into each new system prompt. */
const LEGACY_FILE = 'MEMORY.md';

/** A refused memory operation. The message is what the model (or the person) is told. */
export class MemoryError extends Error {}

/** The file changed after the person opened it for editing. */
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
 * Conversations from before the memory tool keep their frozen prompt, which tells the agent to
 * keep everything in <profile>/MEMORY.md. Whenever that file shows up it is moved into the folder,
 * so nothing an older chat saves gets lost.
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

/**
 * Maps a /memories path onto the folder. Refuses anything that would land outside it, including
 * through a symbolic link inside the folder.
 */
function resolvePath(root: string, memoryPath: unknown, field = 'path'): string {
	if (typeof memoryPath !== 'string' || !memoryPath) refuse(`Error: \`${field}\` is required.`);
	if (memoryPath !== ROOT && !memoryPath.startsWith(`${ROOT}/`)) {
		refuse(`Error: The path ${memoryPath} is outside ${ROOT}. Every path must start with ${ROOT}.`);
	}
	if (memoryPath.includes('\0')) refuse(`Error: The path ${memoryPath} is not valid.`);
	const base = resolve(root);
	const full = resolve(base, memoryPath.slice(ROOT.length).replace(/^\/+/, ''));
	if (full !== base && !full.startsWith(base + sep)) {
		refuse(`Error: The path ${memoryPath} would leave ${ROOT}.`);
	}
	for (let current = full; current !== base; current = dirname(current)) {
		let isLink = false;
		try {
			isLink = lstatSync(current).isSymbolicLink();
		} catch {
			// Doesn't exist (yet).
		}
		if (isLink)
			refuse(`Error: The path ${memoryPath} goes through a link, which memory doesn't follow.`);
	}
	return full;
}

/** The deepest folder the memory tool lists, like its reference implementation. */
const LIST_DEPTH = 2;

function formatSize(bytes: number): string {
	if (bytes < 1024) return `${bytes}B`;
	const units = ['K', 'M', 'G'];
	let size = bytes / 1024;
	let unit = 0;
	while (size >= 1024 && unit < units.length - 1) {
		size /= 1024;
		unit++;
	}
	return `${Number.isInteger(size) ? size : size.toFixed(1)}${units[unit]}`;
}

function numbered(lines: string[], first: number): string {
	return lines.map((line, i) => `${String(first + i).padStart(6)}\t${line}`).join('\n');
}

function listDirectory(full: string, memoryPath: string): string {
	const shownRoot = memoryPath.replace(/\/+$/, '');
	const lines = [`${formatSize(statSync(full).size)}\t${shownRoot}`];
	const walk = (dir: string, rel: string, depth: number) => {
		for (const name of readdirSync(dir).sort()) {
			if (name.startsWith('.') || name === 'node_modules') continue;
			let stat;
			try {
				stat = statSync(join(dir, name));
			} catch {
				continue;
			}
			if (stat.isDirectory()) {
				lines.push(`${formatSize(stat.size)}\t${shownRoot}/${rel}${name}/`);
				if (depth < LIST_DEPTH) walk(join(dir, name), `${rel}${name}/`, depth + 1);
			} else if (stat.isFile()) {
				lines.push(`${formatSize(stat.size)}\t${shownRoot}/${rel}${name}`);
			}
		}
	};
	walk(full, '', 1);
	return `Here're the files and directories up to ${LIST_DEPTH} levels deep in ${shownRoot}, excluding hidden items and node_modules:\n${lines.join('\n')}`;
}

function readTextFile(full: string, memoryPath: string, missing: string): string {
	if (!existsSync(full) || !statSync(full).isFile()) refuse(missing);
	if (IMAGE.test(full))
		refuse(`Error: ${memoryPath} is an image. Memory can only show text files.`);
	return readFileSync(full, 'utf8');
}

function checkSize(text: string, memoryPath: string): void {
	if (text.length > MAX_FILE_CHARS) {
		refuse(
			`Error: ${memoryPath} would be ${text.length} characters; a memory file can have at most ${MAX_FILE_CHARS}. Split it into smaller files by topic, or shorten it.`
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

function lineOf(text: string, index: number): number {
	return text.slice(0, index).split('\n').length;
}

function view(root: string, args: Record<string, unknown>): string {
	const memoryPath = args.path as string;
	const full = resolvePath(root, memoryPath);
	if (!existsSync(full))
		refuse(`The path ${memoryPath} does not exist. Please provide a valid path.`);
	if (statSync(full).isDirectory()) return listDirectory(full, memoryPath);

	const lines = readTextFile(
		full,
		memoryPath,
		`The path ${memoryPath} does not exist. Please provide a valid path.`
	).split('\n');
	if (lines.length > MAX_LINES) {
		refuse(`File ${memoryPath} exceeds maximum line limit of 999,999 lines.`);
	}
	const header = `Here's the content of ${memoryPath} with line numbers:\n`;

	const range = args.view_range;
	if (range !== undefined && range !== null) {
		if (
			!Array.isArray(range) ||
			range.length !== 2 ||
			!range.every((n) => Number.isInteger(n)) ||
			range[0] < 1 ||
			range[0] > lines.length ||
			(range[1] !== -1 && range[1] < range[0])
		) {
			refuse(
				`Error: Invalid \`view_range\` ${JSON.stringify(range)}. ${memoryPath} has ${lines.length} lines; use [start, end] with 1 <= start <= end, or [start, -1] for the rest of the file.`
			);
		}
		const [start, end] = range as [number, number];
		const last = end === -1 ? lines.length : Math.min(end, lines.length);
		return header + numbered(lines.slice(start - 1, last), start);
	}

	let shown = 0;
	let chars = 0;
	while (shown < lines.length && (shown === 0 || chars + lines[shown].length < VIEW_CHARS)) {
		chars += lines[shown].length + 1;
		shown++;
	}
	const cut =
		shown < lines.length
			? `\n[Showing lines 1-${shown} of ${lines.length}: the file is longer than ${VIEW_CHARS} characters. Use view_range to read the rest.]`
			: '';
	return header + numbered(lines.slice(0, shown), 1) + cut;
}

function create(root: string, args: Record<string, unknown>): string {
	const memoryPath = args.path as string;
	const full = resolvePath(root, memoryPath);
	const text = args.file_text;
	if (typeof text !== 'string') refuse('Error: `file_text` must be a string.');
	const existing = existsSync(full) ? statSync(full) : null;
	if (existing?.isDirectory()) refuse(`Error: ${memoryPath} is a directory.`);
	checkSize(text, memoryPath);
	// The tool's description says create "creates or overwrites", so overwriting is expected.
	writeAtomic(full, text);
	return existing
		? `File ${memoryPath} has been overwritten.`
		: `File created successfully at: ${memoryPath}`;
}

function strReplace(root: string, args: Record<string, unknown>): string {
	const memoryPath = args.path as string;
	const full = resolvePath(root, memoryPath);
	const text = readTextFile(
		full,
		memoryPath,
		`Error: The path ${memoryPath} does not exist. Please provide a valid path.`
	);
	const oldStr = args.old_str;
	const newStr = args.new_str ?? '';
	if (typeof oldStr !== 'string' || !oldStr) refuse('Error: `old_str` must be a non-empty string.');
	if (typeof newStr !== 'string') refuse('Error: `new_str` must be a string.');

	const hits: number[] = [];
	for (let at = text.indexOf(oldStr); at !== -1; at = text.indexOf(oldStr, at + 1)) hits.push(at);
	if (hits.length === 0) {
		refuse(
			`No replacement was performed, old_str \`${oldStr}\` did not appear verbatim in ${memoryPath}.`
		);
	}
	if (hits.length > 1) {
		refuse(
			`No replacement was performed. Multiple occurrences of old_str \`${oldStr}\` in lines: ${hits.map((at) => lineOf(text, at)).join(', ')}. Please ensure it is unique`
		);
	}
	// Sliced, not String.replace: `$&` and friends in new_str are meant literally.
	const updated = text.slice(0, hits[0]) + newStr + text.slice(hits[0] + oldStr.length);
	checkSize(updated, memoryPath);
	writeAtomic(full, updated);

	const lines = updated.split('\n');
	const first = lineOf(updated, hits[0]);
	const from = Math.max(1, first - 2);
	const to = Math.min(lines.length, first + newStr.split('\n').length + 1);
	return `The memory file has been edited. Here is the snippet showing the change (with line numbers):\n${numbered(lines.slice(from - 1, to), from)}`;
}

function insert(root: string, args: Record<string, unknown>): string {
	const memoryPath = args.path as string;
	const full = resolvePath(root, memoryPath);
	const text = readTextFile(full, memoryPath, `Error: The path ${memoryPath} does not exist`);
	const line = args.insert_line;
	const insertText = args.insert_text;
	if (typeof insertText !== 'string') refuse('Error: `insert_text` must be a string.');
	const lines = text.split('\n');
	if (typeof line !== 'number' || !Number.isInteger(line) || line < 0 || line > lines.length) {
		refuse(
			`Error: Invalid \`insert_line\` parameter: ${String(line)}. It should be within the range of lines of the file: [0, ${lines.length}]`
		);
	}
	lines.splice(line, 0, insertText.replace(/\n$/, ''));
	const updated = lines.join('\n');
	checkSize(updated, memoryPath);
	writeAtomic(full, updated);
	return `The file ${memoryPath} has been edited.`;
}

function remove(root: string, args: Record<string, unknown>): string {
	const memoryPath = args.path as string;
	const full = resolvePath(root, memoryPath);
	if (full === resolve(root)) refuse(`Error: The ${ROOT} directory itself cannot be deleted.`);
	if (!existsSync(full)) refuse(`Error: The path ${memoryPath} does not exist`);
	rmSync(full, { recursive: true });
	return `Successfully deleted ${memoryPath}`;
}

function rename(root: string, args: Record<string, unknown>): string {
	const oldPath = args.old_path as string;
	const newPath = args.new_path as string;
	const from = resolvePath(root, oldPath, 'old_path');
	const to = resolvePath(root, newPath, 'new_path');
	const base = resolve(root);
	if (from === base || to === base)
		refuse(`Error: The ${ROOT} directory itself cannot be renamed.`);
	if (!existsSync(from)) refuse(`Error: The path ${oldPath} does not exist`);
	if (existsSync(to)) refuse(`Error: The destination ${newPath} already exists`);
	if (to.startsWith(from + sep)) refuse(`Error: ${oldPath} cannot be moved into itself.`);
	mkdirSync(dirname(to), { recursive: true, mode: 0o700 });
	renameSync(from, to);
	return `Successfully renamed ${oldPath} to ${newPath}`;
}

const COMMANDS: Record<string, (root: string, args: Record<string, unknown>) => string> = {
	view,
	create,
	str_replace: strReplace,
	insert,
	delete: remove,
	rename
};

/** Runs one memory tool call on the profile's memory folder. */
export function runMemoryCommand(
	slug: string,
	input: unknown
): { content: string; isError: boolean } {
	try {
		if (!input || typeof input !== 'object') refuse('Error: The input must be an object.');
		const args = input as Record<string, unknown>;
		const name = args.command;
		if (typeof name !== 'string' || !Object.hasOwn(COMMANDS, name)) {
			refuse(
				`Error: Unknown command ${JSON.stringify(name)}. Use one of: ${Object.keys(COMMANDS).join(', ')}.`
			);
		}
		return { content: COMMANDS[name](openMemory(slug), args), isError: false };
	} catch (err) {
		if (err instanceof MemoryError) return { content: err.message, isError: true };
		return { content: `Error: ${err instanceof Error ? err.message : String(err)}`, isError: true };
	}
}

// --- The Memory page ---

export interface MemoryFile {
	/** Relative to /memories, like `family.md` or `people/anna.md`. */
	path: string;
	text: string;
	size: number;
	/** Modification time in ms. Also the version an edit is based on. */
	updatedAt: number;
}

/** Deeper than the tool lists, so nothing the agent nests is hidden from the family. */
const PAGE_DEPTH = 4;
/** The tool can't write files this big; only something copied in by hand can be. */
const PAGE_MAX_BYTES = 1_000_000;

/** Every text file in the profile's memory, sorted by path. */
export function listMemoryFiles(slug: string): MemoryFile[] {
	const root = openMemory(slug);
	const files: MemoryFile[] = [];
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
				if (depth < PAGE_DEPTH) walk(full, depth + 1);
			} else if (stat.isFile() && !IMAGE.test(name) && stat.size <= PAGE_MAX_BYTES) {
				files.push({
					path: relative(root, full).split(sep).join('/'),
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

function pagePath(root: string, path: string): string {
	const full = resolvePath(root, `${ROOT}/${path.replace(/^\/+/, '')}`);
	if (full === resolve(root)) refuse('Error: Pick a file.');
	return full;
}

/**
 * Saves a file someone edited on the Memory page. `basedOn` is the `updatedAt` they started from;
 * if the agent changed the file since, nothing is written.
 */
export function writeMemoryFile(slug: string, path: string, text: string, basedOn: number): void {
	const root = openMemory(slug);
	const full = pagePath(root, path);
	if (!existsSync(full) || !statSync(full).isFile()) {
		throw new MemoryConflictError(`${path} was deleted while you were editing it.`);
	}
	if (statSync(full).mtimeMs !== basedOn) {
		throw new MemoryConflictError(`btw changed ${path} while you were editing it.`);
	}
	checkSize(text, path);
	writeAtomic(full, text);
}

export function forgetMemoryFile(slug: string, path: string): void {
	const root = openMemory(slug);
	const full = pagePath(root, path);
	if (existsSync(full) && statSync(full).isFile()) unlinkSync(full);
}
