import {
	MAX_PINNED_CHARS,
	addMemoryFact,
	forgetMemoryFact,
	formatLocalTime,
	isPinnedNote,
	listMemoryFiles,
	profileMemoryDir,
	readMemoryNote,
	removeMemoryNote,
	renameMemoryNote,
	replaceInMemory,
	writeMemoryNote
} from '@nolune/core';
import type { Io } from './io.ts';
import { profileFor } from './profile.ts';

export const MEMORY_HELP = `Memory (short notes per topic; the agent reads the ones it needs)
  nolune memory [list] [--profile SLUG]      the notes and how many facts each holds
  nolune memory show <topic>...              print notes (a topic is family, people/anna, …)
  nolune memory add <topic> <fact>           add one fact; the note is created if needed
  nolune memory replace <topic> <old> <new>  change text that appears once in the note
  nolune memory forget <topic> <text>        remove the one line that contains <text>
  nolune memory write <topic> [text]         replace the whole note (the text, or stdin)
  nolune memory rm <topic>
  nolune memory mv <topic> <new-topic>
  The note core is pinned: every new chat starts with it, so it holds at most ${MAX_PINNED_CHARS} characters.`;

/**
 * `--profile` is picked out by hand: facts are free text and may start with a dash, which an
 * option parser would take for an option. `nolune soul` does the same.
 */
export function splitProfile(args: string[]): { profile: string | undefined; words: string[] } {
	let profile: string | undefined;
	const words: string[] = [];
	for (let i = 0; i < args.length; i++) {
		const arg = args[i];
		if (arg === '--') {
			words.push(...args.slice(i + 1));
			break;
		}
		if (arg === '--profile') profile = args[++i];
		else if (arg.startsWith('--profile=')) profile = arg.slice('--profile='.length);
		else words.push(arg);
	}
	return { profile, words };
}

function need(words: string[], count: number, usage: string): void {
	if (words.length < count) throw new Error(`usage: nolune memory ${usage}`);
}

function plural(n: number, one: string, many: string): string {
	return `${n} ${n === 1 ? one : many}`;
}

export async function memoryCommand(io: Io, args: string[]): Promise<void> {
	const { profile: flag, words } = splitProfile(args);
	const [action = 'list', ...rest] = words;
	if (action === 'help') {
		io.log(MEMORY_HELP);
		return;
	}
	const profile = profileFor(io, flag);
	const slug = profile.slug;

	switch (action) {
		case 'list': {
			const files = listMemoryFiles(slug);
			if (!files.length) {
				io.log(
					`No notes yet for ${profile.name}. Start one with: nolune memory add <topic> "<fact>"`
				);
				return;
			}
			io.log(`Notes for ${profile.name} (${profileMemoryDir(slug)}):`);
			const width = Math.max(...files.map((f) => f.path.length));
			for (const f of files) {
				io.log(
					`  ${f.path.padEnd(width)}  ${plural(f.facts.length, 'fact', 'facts').padEnd(9)}  changed ${formatLocalTime(new Date(f.updatedAt))}${isPinnedNote(f.path) ? '  (pinned: in every new chat)' : ''}`
				);
			}
			return;
		}
		case 'show': {
			need(rest, 1, 'show <topic>...');
			for (const [i, topic] of rest.entries()) {
				const note = readMemoryNote(slug, topic);
				if (rest.length > 1) io.log(`${i ? '\n' : ''}==> ${note.path} <==`);
				io.stdout(note.text.endsWith('\n') ? note.text : `${note.text}\n`);
			}
			return;
		}
		case 'add': {
			need(rest, 2, 'add <topic> <fact>');
			const [topic, ...fact] = rest;
			const result = addMemoryFact(slug, topic, fact.join(' '));
			if (result.duplicate) io.log(`${result.path} already has that.`);
			else io.log(`${result.created ? 'Started' : 'Saved to'} ${result.path}.`);
			return;
		}
		case 'replace': {
			need(rest, 3, 'replace <topic> <old text> <new text>');
			const { path } = replaceInMemory(slug, rest[0], rest[1], rest[2]);
			io.log(`Updated ${path}.`);
			return;
		}
		case 'forget': {
			need(rest, 2, 'forget <topic> <text>');
			const [topic, ...text] = rest;
			const { path, removed } = forgetMemoryFact(slug, topic, text.join(' '));
			io.log(`Removed from ${path}: ${removed}`);
			return;
		}
		case 'write': {
			need(rest, 1, 'write <topic> [text]   (without text, the note is read from stdin)');
			const [topic, ...text] = rest;
			if (!text.length && io.stdinIsTTY) {
				throw new Error(
					'give the note as text, or pipe it in: nolune memory write <topic> < note.md'
				);
			}
			const { path, created } = writeMemoryNote(
				slug,
				topic,
				text.length ? text.join(' ') : await io.readStdin()
			);
			io.log(`${created ? 'Created' : 'Rewrote'} ${path}.`);
			return;
		}
		case 'rm': {
			need(rest, 1, 'rm <topic>');
			io.log(`Deleted ${removeMemoryNote(slug, rest[0]).path}.`);
			return;
		}
		case 'mv': {
			need(rest, 2, 'mv <topic> <new topic>');
			const moved = renameMemoryNote(slug, rest[0], rest[1]);
			io.log(`Renamed ${moved.from} to ${moved.to}.`);
			return;
		}
		default:
			throw new Error(`unknown memory command "${action}". See \`nolune memory help\`.`);
	}
}
