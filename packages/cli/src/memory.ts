import { readFileSync } from 'node:fs';
import {
	MAX_PINNED_CHARS,
	addMemoryFact,
	forgetMemoryFact,
	formatLocalTime,
	getProfileBySlug,
	isPinnedNote,
	listMemoryFiles,
	profileMemoryDir,
	readMemoryNote,
	removeMemoryNote,
	renameMemoryNote,
	replaceInMemory,
	writeMemoryNote,
	type Profile
} from '@btw/core';

export const MEMORY_HELP = `Memory (short notes per topic; the agent reads the ones it needs)
  btw memory [list] [--profile SLUG]         the notes and how many facts each holds
  btw memory show <topic>...                 print notes (a topic is family, people/anna, …)
  btw memory add <topic> <fact>              add one fact; the note is created if needed
  btw memory replace <topic> <old> <new>     change text that appears once in the note
  btw memory forget <topic> <text>           remove the one line that contains <text>
  btw memory write <topic> [text]            replace the whole note (the text, or stdin)
  btw memory rm <topic>
  btw memory mv <topic> <new-topic>
  The note core is pinned: every new chat starts with it, so it holds at most ${MAX_PINNED_CHARS} characters.`;

/**
 * `--profile` is picked out by hand: facts are free text and may start with a dash, which an
 * option parser would take for an option. `btw soul` does the same.
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

export function profileFor(flag: string | undefined): Profile {
	const slug = flag || process.env.BTW_PROFILE;
	if (!slug) throw new Error('which profile? Pass --profile <slug>. See `btw profile list`.');
	const found = getProfileBySlug(slug);
	if (!found) throw new Error(`no profile with slug "${slug}". See \`btw profile list\`.`);
	return found;
}

function need(words: string[], count: number, usage: string): void {
	if (words.length < count) throw new Error(`usage: btw memory ${usage}`);
}

function plural(n: number, one: string, many: string): string {
	return `${n} ${n === 1 ? one : many}`;
}

export function memoryCommand(args: string[]): void {
	const { profile: flag, words } = splitProfile(args);
	const [action = 'list', ...rest] = words;
	if (action === 'help') {
		console.log(MEMORY_HELP);
		return;
	}
	const profile = profileFor(flag);
	const slug = profile.slug;

	switch (action) {
		case 'list': {
			const files = listMemoryFiles(slug);
			if (!files.length) {
				console.log(
					`No notes yet for ${profile.name}. Start one with: btw memory add <topic> "<fact>"`
				);
				return;
			}
			console.log(`Notes for ${profile.name} (${profileMemoryDir(slug)}):`);
			const width = Math.max(...files.map((f) => f.path.length));
			for (const f of files) {
				console.log(
					`  ${f.path.padEnd(width)}  ${plural(f.facts.length, 'fact', 'facts').padEnd(9)}  changed ${formatLocalTime(new Date(f.updatedAt))}${isPinnedNote(f.path) ? '  (pinned: in every new chat)' : ''}`
				);
			}
			return;
		}
		case 'show': {
			need(rest, 1, 'show <topic>...');
			for (const [i, topic] of rest.entries()) {
				const note = readMemoryNote(slug, topic);
				if (rest.length > 1) console.log(`${i ? '\n' : ''}==> ${note.path} <==`);
				process.stdout.write(note.text.endsWith('\n') ? note.text : `${note.text}\n`);
			}
			return;
		}
		case 'add': {
			need(rest, 2, 'add <topic> <fact>');
			const [topic, ...fact] = rest;
			const result = addMemoryFact(slug, topic, fact.join(' '));
			if (result.duplicate) console.log(`${result.path} already has that.`);
			else console.log(`${result.created ? 'Started' : 'Saved to'} ${result.path}.`);
			return;
		}
		case 'replace': {
			need(rest, 3, 'replace <topic> <old text> <new text>');
			const { path } = replaceInMemory(slug, rest[0], rest[1], rest[2]);
			console.log(`Updated ${path}.`);
			return;
		}
		case 'forget': {
			need(rest, 2, 'forget <topic> <text>');
			const [topic, ...text] = rest;
			const { path, removed } = forgetMemoryFact(slug, topic, text.join(' '));
			console.log(`Removed from ${path}: ${removed}`);
			return;
		}
		case 'write': {
			need(rest, 1, 'write <topic> [text]   (without text, the note is read from stdin)');
			const [topic, ...text] = rest;
			if (!text.length && process.stdin.isTTY) {
				throw new Error('give the note as text, or pipe it in: btw memory write <topic> < note.md');
			}
			const { path, created } = writeMemoryNote(
				slug,
				topic,
				text.length ? text.join(' ') : readFileSync(0, 'utf8')
			);
			console.log(`${created ? 'Created' : 'Rewrote'} ${path}.`);
			return;
		}
		case 'rm': {
			need(rest, 1, 'rm <topic>');
			console.log(`Deleted ${removeMemoryNote(slug, rest[0]).path}.`);
			return;
		}
		case 'mv': {
			need(rest, 2, 'mv <topic> <new topic>');
			const moved = renameMemoryNote(slug, rest[0], rest[1]);
			console.log(`Renamed ${moved.from} to ${moved.to}.`);
			return;
		}
		default:
			throw new Error(`unknown memory command "${action}". See \`btw memory help\`.`);
	}
}
