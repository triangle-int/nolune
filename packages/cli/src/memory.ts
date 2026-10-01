import {
	MAX_CARD_CHARS,
	MAX_PINNED_CHARS,
	MEMORY_CATEGORIES,
	addMemoryFact,
	addToCard,
	cardFiles,
	categoryOf,
	checkCardWrite,
	factLines,
	forgetInCard,
	forgetMemoryFact,
	formatLocalTime,
	isCardPath,
	isPinnedNote,
	listMemoryFiles,
	membersWithNotes,
	mergeProfileNotes,
	moveProfileNote,
	paths,
	profileCard,
	profileCards,
	profileMemoryDir,
	readCard,
	readMemoryNote,
	recordAgentCardChanges,
	removeMemoryNote,
	replaceInCard,
	replaceInMemory,
	searchMemory,
	setLearnFromChats,
	writeMemoryNote,
	type Card,
	type Profile
} from '@nolune/core';
import type { Io } from './io.ts';
import { profileFor } from './profile.ts';

export const MEMORY_HELP = `Memory (short notes in fixed categories; the agent reads the ones it needs)
  nolune memory [list] [--profile SLUG]         the notes, how many facts each holds, and whose they are
  nolune memory search <words>...               find facts in every note, best match first
  nolune memory show <topic>...                 print notes (a topic is home, people/anna, …)
  nolune memory add <topic> <fact>              add one fact; the note is created if needed
  nolune memory replace <topic> <old> <new>     change text that appears once in the note
  nolune memory forget <topic> <text>           remove the one line that contains <text>
  nolune memory write <topic> [text]            replace the whole note (the text, or stdin)
  nolune memory rm <topic>
  nolune memory mv <topic> <new-topic>
  nolune memory merge <topic> <into-topic>      put one note into another about the same thing
  nolune memory learning [on|off]               whether nolune also saves what it learns by itself,
                                             looking over each chat once it goes quiet
  Categories: ${MEMORY_CATEGORIES.map((c) => (c === 'people' || c === 'projects' ? `${c}/<name>` : c)).join(', ')}.
  Facts go only into these; a note from before them can be read and rewritten until it's moved.
  The note core is pinned: every new chat starts with it, so it holds at most ${MAX_PINNED_CHARS} characters.
  Each member also has a card, cards/<name>: a note about them that goes with them into all their
  profiles (at most ${MAX_CARD_CHARS} characters). show, search, add, replace and forget work on it; a change
  is refused unless its owner wrote one of the messages the agent is answering.`;

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

/** Why `nolune memory <action>` doesn't take a card. */
function notOnCards(action: string, topics: string[]): void {
	const card = topics.find(isCardPath);
	if (!card) return;
	throw new Error(
		action === 'write'
			? `${card} is a card: change it a fact at a time with add, replace and forget. Only its owner rewrites it, on its page.`
			: `${card} is a card, which isn't ${action === 'rm' ? 'deleted' : 'moved or merged'}: change it a fact at a time with add, replace and forget.`
	);
}

/** A card of the profile's, after checking the agent of this chat may change it. */
function writableCard(io: Io, profile: Profile, topic: string): Card {
	const card = profileCard(profile, topic);
	checkCardWrite(card, {
		profileId: profile.id,
		conversationId: io.env.NOLUNE_CONVERSATION_ID
	});
	return card;
}

/** Records the agent's change to a card for its owner, when it runs in a chat. */
function recordCardChange(
	io: Io,
	profile: Profile,
	change: Parameters<typeof recordAgentCardChanges>[1][number]
): void {
	const conversationId = io.env.NOLUNE_CONVERSATION_ID;
	if (conversationId) recordAgentCardChanges({ profileId: profile.id, conversationId }, [change]);
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
				listCards(io, profileCards(profile.id));
				return;
			}
			const members = membersWithNotes(profile);
			const whose = new Map(members.flatMap((m) => (m.note ? [[m.note, m.name] as const] : [])));
			io.log(`Notes for ${profile.name} (${profileMemoryDir(slug)}):`);
			const width = Math.max(...files.map((f) => f.path.length));
			for (const f of files) {
				const about = isPinnedNote(f.path)
					? '  (pinned: in every new chat)'
					: whose.has(f.path)
						? `  (${whose.get(f.path)}'s note)`
						: categoryOf(f.path)
							? ''
							: '  (from before the categories: move it into one)';
				io.log(
					`  ${f.path.padEnd(width)}  ${plural(f.facts.length, 'fact', 'facts').padEnd(9)}  changed ${formatLocalTime(new Date(f.updatedAt))}${about}`
				);
			}
			const waiting = members.filter((m) => !m.note || !files.some((f) => f.path === m.note));
			for (const m of waiting) {
				io.log(
					m.note
						? `  ${m.name}'s note ${m.note} is started when there is something to write.`
						: `  Which note is ${m.name}'s isn't known yet: maybe ${m.candidates.map((c) => c.path).join(', ')}. Link it in the profile's settings.`
				);
			}
			listCards(io, profileCards(profile.id));
			return;
		}
		case 'search': {
			need(rest, 1, 'search <words>...');
			const query = rest.join(' ');
			const cards = profileCards(profile.id).map((card) => card.path);
			const hits = await searchMemory(slug, query, 20, cards);
			if (!hits.length) {
				io.log(
					`Nothing in ${profile.name}'s memory matches "${query}". Try other words (or another language), or read a note with \`nolune memory show <topic>\`.`
				);
				return;
			}
			for (const hit of hits) {
				io.log(`${hit.path}:${hit.line}  ${hit.text}${hit.heading ? `  (${hit.heading})` : ''}`);
			}
			return;
		}
		case 'show': {
			need(rest, 1, 'show <topic>...');
			for (const [i, topic] of rest.entries()) {
				const note = isCardPath(topic)
					? cardNote(profileCard(profile, topic))
					: readMemoryNote(slug, topic);
				if (rest.length > 1) io.log(`${i ? '\n' : ''}==> ${note.path} <==`);
				io.stdout(note.text.endsWith('\n') ? note.text : `${note.text}\n`);
			}
			return;
		}
		case 'add': {
			need(rest, 2, 'add <topic> <fact>');
			const [topic, ...fact] = rest;
			const card = isCardPath(topic) ? writableCard(io, profile, topic) : null;
			// A quoted list is a fact a line.
			const lines = factLines(fact.join(' '));
			const results = (lines.length ? lines : ['']).map((line) =>
				card ? addToCard(card, line) : addMemoryFact(slug, topic, line)
			);
			if (card) {
				for (const r of results.filter((r) => !r.duplicate)) {
					recordCardChange(io, profile, { op: 'add', note: r.path, line: r.line });
				}
			}
			const { path } = results[0];
			const saved = results.filter((r) => !r.duplicate).length;
			const verb = results.some((r) => r.created) ? 'Started' : 'Saved to';
			if (!saved) io.log(`${path} already has ${results.length === 1 ? 'that' : 'those'}.`);
			else if (results.length === 1) io.log(`${verb} ${path}.`);
			else {
				const had = results.length - saved;
				io.log(
					`${verb} ${path}: ${plural(saved, 'fact', 'facts')}${had ? `, and ${had} it already had` : ''}.`
				);
			}
			return;
		}
		case 'replace': {
			need(rest, 3, 'replace <topic> <old text> <new text>');
			if (isCardPath(rest[0])) {
				const card = writableCard(io, profile, rest[0]);
				const replaced = replaceInCard(card, rest[1], rest[2]);
				recordCardChange(io, profile, {
					op: 'replace',
					note: replaced.path,
					line: replaced.after,
					before: replaced.before
				});
				io.log(`Updated ${replaced.path}, ${card.owner}'s card.`);
				return;
			}
			const { path } = replaceInMemory(slug, rest[0], rest[1], rest[2]);
			io.log(`Updated ${path}.`);
			return;
		}
		case 'forget': {
			need(rest, 2, 'forget <topic> <text>');
			const [topic, ...text] = rest;
			const { path, removed } = isCardPath(topic)
				? forgetInCard(writableCard(io, profile, topic), text.join(' '))
				: forgetMemoryFact(slug, topic, text.join(' '));
			io.log(`Removed from ${path}: ${removed}`);
			return;
		}
		case 'write': {
			need(rest, 1, 'write <topic> [text]   (without text, the note is read from stdin)');
			notOnCards('write', rest.slice(0, 1));
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
			notOnCards('rm', rest.slice(0, 1));
			io.log(`Deleted ${removeMemoryNote(slug, rest[0]).path}.`);
			return;
		}
		case 'mv': {
			need(rest, 2, 'mv <topic> <new topic>');
			notOnCards('mv', rest.slice(0, 2));
			const moved = moveProfileNote(profile, rest[0], rest[1]);
			io.log(`Renamed ${moved.from} to ${moved.to}.`);
			return;
		}
		case 'merge': {
			need(rest, 2, 'merge <topic> <into topic>');
			notOnCards('merge', rest.slice(0, 2));
			const merged = mergeProfileNotes(profile, rest[0], rest[1]);
			io.log(
				merged.merged
					? `Merged ${merged.from} into ${merged.into}: ${plural(merged.added, 'part', 'parts')} it didn't have.`
					: `There was no ${merged.into} yet, so ${merged.from} was renamed to it.`
			);
			return;
		}
		case 'learning': {
			const [value] = rest;
			if (value !== undefined && value !== 'on' && value !== 'off') {
				throw new Error('usage: nolune memory learning [on|off]');
			}
			const on = value === undefined ? profile.learnFromChats : value === 'on';
			if (value !== undefined) setLearnFromChats(profile.id, on);
			io.log(
				on
					? `nolune looks over ${profile.name}'s chats once they go quiet and saves what's worth remembering.`
					: `nolune saves to ${profile.name}'s memory only when it thinks of it in a chat.`
			);
			return;
		}
		default:
			throw new Error(`unknown memory command "${action}". See \`nolune memory help\`.`);
	}
}

/** A card as `show` prints it: an empty one says so. */
function cardNote(card: Card): { path: string; text: string } {
	return (
		readCard(card) ?? {
			path: card.path,
			text: `# ${card.owner}\n\n(${card.owner}'s card is empty so far.)\n`
		}
	);
}

/** The members' cards, after a profile's notes. */
function listCards(io: Io, cards: Card[]): void {
	if (!cards.length) return;
	const files = new Map(cardFiles(cards).map((file) => [file.path, file]));
	io.log(
		`Cards, which go with each member into all their profiles (${paths.cards}), at most ${MAX_CARD_CHARS} characters each:`
	);
	const width = Math.max(...cards.map((card) => card.path.length));
	for (const card of cards) {
		const file = files.get(card.path);
		const facts = file?.facts.length ?? 0;
		io.log(
			`  ${card.path.padEnd(width)}  ${plural(facts, 'fact', 'facts').padEnd(9)}  ${file && facts ? `changed ${formatLocalTime(new Date(file.updatedAt))}  ` : ''}(${card.owner}'s)`
		);
	}
}
