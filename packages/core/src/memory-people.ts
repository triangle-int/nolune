import { and, eq } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { memoryChange, profileMember, user } from './db/schema.ts';
import {
	aliasesOf,
	categoryOf,
	isAliasLine,
	nameKey,
	noteName,
	titleIn,
	whoIn
} from './memory-categories.ts';
import { profileCards } from './memory-cards.ts';
import { parseFacts } from './memory-facts.ts';
import {
	MemoryError,
	listMemoryNotes,
	mergeMemoryNotes,
	readMemoryNote,
	readMemoryNotes,
	renameMemoryNote
} from './memory.ts';
import { addMember, isMember, type Profile } from './profiles.ts';
import { findUser } from './users.ts';

/*
 * Which note in memory is about which member of the profile, so nolune knows who "I" is in a
 * message, what to look up for them, and where to write what they tell it about themselves.
 * Memory has notes about people who aren't members too (grandparents, the nanny), and one may be
 * about someone before they join: then whoever adds them is asked whether it is them, with a look
 * at what it says, since they will be able to read it. It's never linked without asking. Once
 * nothing in memory is about a member, the note their facts will start is kept for them. Moving
 * or merging a note takes its link along, and what the note-taker saved in it (memory-changes).
 */

type ProfileRef = Pick<Profile, 'id' | 'slug'>;

/** A note about a person, as the Memory page and the settings show it. */
export interface PersonNote {
	/** Like `people/anna.md`. */
	path: string;
	title: string | null;
	/** Who they are to the family, from its "Who:" line. */
	who: string | null;
	/** What the family calls them, from its "Also called:" line. */
	aliases: string[];
	/** Its other facts, as plain text. */
	facts: string[];
}

function personNote(path: string, text: string): PersonNote {
	return {
		path,
		title: titleIn(text),
		who: whoIn(text),
		aliases: aliasesOf(text),
		facts: parseFacts(text).filter((fact) => !isAliasLine(fact) && !whoIn(fact))
	};
}

/** Every person's note, in people/, sorted by path. */
export function listPersonNotes(slug: string): PersonNote[] {
	return readMemoryNotes(slug)
		.filter((note) => categoryOf(note.path) === 'people')
		.map((note) => personNote(note.path, note.text));
}

/** A name as compared: all of it, and the first name. */
function keysOf(name: string): string[] {
	const first = name.trim().split(/\s+/)[0] ?? '';
	return [...new Set([nameKey(name), nameKey(first)])].filter(Boolean);
}

/** What a note about someone calls them: its name, its title, its "Also called". */
function noteKeys(note: PersonNote): string[] {
	const stem = note.path.replace(/^people\//, '').replace(/\.(md|markdown|txt)$/i, '');
	const names = [stem.replace(/[-_]+/g, ' '), note.title, ...note.aliases];
	return [...new Set(names.flatMap((name) => (name ? keysOf(name) : [])))];
}

/** Names a letter apart, long enough for that to be spelling: Yulia and Yulya (Юля). */
function close(a: string, b: string): boolean {
	if (Math.min(a.length, b.length) < 5 || Math.abs(a.length - b.length) > 1) return false;
	let i = 0;
	while (i < a.length && a[i] === b[i]) i++;
	return (
		a.slice(i + 1) === b.slice(i + 1) ||
		a.slice(i) === b.slice(i + 1) ||
		a.slice(i + 1) === b.slice(i)
	);
}

/**
 * The notes that may be about someone called `name`, best first: by the full name, then the first
 * name, then a spelling a letter apart, in any alphabet (Ольга is Olga).
 */
export function personNoteCandidates(
	slug: string,
	name: string,
	notes = listPersonNotes(slug)
): PersonNote[] {
	const [full, first] = [nameKey(name), keysOf(name).at(-1) ?? ''];
	return notes
		.map((note) => {
			const keys = noteKeys(note);
			const score = keys.includes(full)
				? 3
				: keys.includes(first)
					? 2
					: keys.some((key) => close(key, full) || close(key, first))
						? 1
						: 0;
			return { note, score };
		})
		.filter((found) => found.score)
		.sort((a, b) => b.score - a.score || a.note.path.localeCompare(b.note.path))
		.map((found) => found.note);
}

/** The members' notes: path → member. */
function linkedNotes(profileId: string): Map<string, { id: string; name: string }> {
	const rows = getDb()
		.select({ id: user.id, name: user.name, note: profileMember.personNote })
		.from(profileMember)
		.innerJoin(user, eq(user.id, profileMember.userId))
		.where(eq(profileMember.profileId, profileId))
		.all();
	return new Map(rows.flatMap((row) => (row.note ? [[row.note, row] as const] : [])));
}

/** A note for someone that nothing is in yet: `people/anna.md`, or `people/anna-smith.md`... */
function freshPersonNote(slug: string, name: string, taken: Iterable<string>): string {
	const used = new Set([...listMemoryNotes(slug), ...taken].map((path) => path.toLowerCase()));
	const first = noteName(name.trim().split(/\s+/)[0] ?? name);
	for (const base of [first, noteName(name)]) {
		if (!used.has(`people/${base}.md`)) return `people/${base}.md`;
	}
	for (let i = 2; ; i++) {
		if (!used.has(`people/${first}-${i}.md`)) return `people/${first}-${i}.md`;
	}
}

/** `note` as a person's note to link, or why it can't be. */
function linkable(note: string, linked: Map<string, { id: string; name: string }>, userId: string) {
	const clean = note.trim().replace(/^\/+/, '');
	const path = /\.(md|markdown|txt)$/i.test(clean) ? clean : `${clean}.md`;
	if (categoryOf(path) !== 'people' || !/^people\/[^/.][^/]*$/.test(path)) {
		throw new MemoryError(
			`"${note}" isn't a person's note: those are in people/, like people/anna.`
		);
	}
	const holder = linked.get(path);
	if (holder && holder.id !== userId) throw new MemoryError(`${path} is ${holder.name}'s note.`);
	return path;
}

function setNote(profileId: string, userId: string, note: string | null): void {
	getDb()
		.update(profileMember)
		.set({ personNote: note })
		.where(and(eq(profileMember.profileId, profileId), eq(profileMember.userId, userId)))
		.run();
}

export interface MemberNote {
	id: string;
	name: string;
	/** Their note, like `people/anna.md`; null while it isn't known which one. */
	note: string | null;
	/** Whether it's there yet: it's started when there is something to write. */
	exists: boolean;
	/** While `note` is null: the notes that may be about them, to ask. */
	candidates: PersonNote[];
}

/**
 * The members with their notes, by name. A member nothing in memory may be about gets the note
 * their facts will start; one it may be is left for someone to say which (linkPersonNote).
 */
export function membersWithNotes(profile: ProfileRef): MemberNote[] {
	const rows = getDb()
		.select({ id: user.id, name: user.name, note: profileMember.personNote })
		.from(profileMember)
		.innerJoin(user, eq(user.id, profileMember.userId))
		.where(eq(profileMember.profileId, profile.id))
		.orderBy(user.name)
		.all();
	const notes = listPersonNotes(profile.slug);
	const present = new Set(notes.map((note) => note.path));
	const linked = new Set(rows.flatMap((row) => row.note ?? []));
	const free = notes.filter((note) => !linked.has(note.path));
	return rows.map((row) => {
		if (row.note) return { ...row, exists: present.has(row.note), candidates: [] };
		const candidates = personNoteCandidates(profile.slug, row.name, free);
		if (candidates.length) return { ...row, exists: false, candidates };
		const note = freshPersonNote(profile.slug, row.name, linked);
		setNote(profile.id, row.id, note);
		linked.add(note);
		return { ...row, note, exists: false, candidates };
	});
}

/**
 * Says which note is about a member: a person's note (there already or not), or 'new' for one
 * nothing is in yet.
 */
export function linkPersonNote(profile: ProfileRef, userId: string, note: string): string {
	const linked = linkedNotes(profile.id);
	const path =
		note === 'new'
			? freshPersonNote(
					profile.slug,
					getDb().select({ name: user.name }).from(user).where(eq(user.id, userId)).get()?.name ??
						'someone',
					linked.keys()
				)
			: linkable(note, linked, userId);
	setNote(profile.id, userId, path);
	return path;
}

export type AddedMember =
	| { added: true; name: string; note: string }
	| { added: false; name: string; candidates: PersonNote[] };

/**
 * Adds someone to the profile with their note: `note` as linkPersonNote takes it. Without one,
 * when memory has notes that may be about them, they aren't added yet: those come back, to ask
 * which is theirs.
 */
export function addMemberWithNote(
	profile: ProfileRef,
	nameOrEmail: string,
	note?: string
): AddedMember {
	const found = findUser(nameOrEmail.trim());
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	if (isMember(profile.id, found.id)) throw new Error(`${found.name} is already a member`);
	const linked = linkedNotes(profile.id);
	if (!note) {
		const free = listPersonNotes(profile.slug).filter((n) => !linked.has(n.path));
		const candidates = personNoteCandidates(profile.slug, found.name, free);
		if (candidates.length) return { added: false, name: found.name, candidates };
	}
	const path =
		!note || note === 'new'
			? freshPersonNote(profile.slug, found.name, linked.keys())
			: linkable(note, linked, found.id);
	addMember(profile.id, found.email, path);
	return { added: true, name: found.name, note: path };
}

/** After a note moved or was merged into another: its link and changes go along. */
function followNote(profileId: string, from: string, to: string, merged: boolean): void {
	getDb().transaction((tx) => {
		tx.update(profileMember)
			.set({ personNote: categoryOf(to) === 'people' ? to : null })
			.where(and(eq(profileMember.profileId, profileId), eq(profileMember.personNote, from)))
			.run();
		// A merged note's changes are in the other now; undoing one never deletes that note.
		tx.update(memoryChange)
			.set(merged ? { note: to, createdNote: false } : { note: to })
			.where(and(eq(memoryChange.profileId, profileId), eq(memoryChange.note, from)))
			.run();
	});
}

/** renameMemoryNote, taking along the note's link and changes. */
export function moveProfileNote(
	profile: ProfileRef,
	from: string,
	to: string
): { from: string; to: string } {
	const moved = renameMemoryNote(profile.slug, from, to);
	followNote(profile.id, moved.from, moved.to, false);
	return moved;
}

/**
 * mergeMemoryNotes, taking along the note's link and changes. Refuses to merge two members'
 * notes: they're about two people.
 */
export function mergeProfileNotes(
	profile: ProfileRef,
	from: string,
	into: string
): { from: string; into: string; added: number; merged: boolean } {
	const linked = linkedNotes(profile.id);
	const path = (topic: string) => {
		const clean = topic.trim().replace(/^\/+|\/+$/g, '');
		return /\.(md|markdown|txt)$/i.test(clean) ? clean : `${clean}.md`;
	};
	const [a, b] = [linked.get(path(from)), linked.get(path(into))];
	if (a && b && a.id !== b.id) {
		throw new MemoryError(
			`${path(from)} is ${a.name}'s note and ${path(into)} is ${b.name}'s: they're about two people.`
		);
	}
	const merged = mergeMemoryNotes(profile.slug, from, into);
	followNote(profile.id, merged.from, merged.into, true);
	return merged;
}

/**
 * Who is who, for the agent and the note-taker: each member with their card (memory-cards.ts)
 * and their note here, like `- Anna Smith: card cards/anna, note people/anna`. Empty without
 * members.
 */
export function peopleGuide(profile: ProfileRef): string {
	const cards = new Map(profileCards(profile.id).map((card) => [card.userId, card]));
	return membersWithNotes(profile)
		.map((member) => {
			const note = member.note?.replace(/\.md$/i, '');
			const state = !note
				? 'no note here linked yet'
				: member.exists
					? `note ${note}`
					: `note ${note} (nothing in it yet)`;
			const card = cards.get(member.id)?.path.replace(/\.md$/i, '');
			return `- ${member.name}: ${card ? `card ${card}, ` : ''}${state}`;
		})
		.join('\n');
}

/**
 * What a member goes by in memory, to prefer facts about them: their name, and their note's name,
 * title and "Also called". Just the name without a note, or when it can't be read.
 */
export function memberWords(profile: ProfileRef, userId: string, name: string): string {
	const row = getDb()
		.select({ note: profileMember.personNote })
		.from(profileMember)
		.where(and(eq(profileMember.profileId, profile.id), eq(profileMember.userId, userId)))
		.get();
	if (!row?.note) return name;
	let text = '';
	try {
		text = readMemoryNote(profile.slug, row.note).text;
	} catch {
		// Not started yet.
	}
	const stem = row.note.replace(/^people\//, '').replace(/\.(md|markdown|txt)$/i, '');
	return [name, stem.replace(/[-_]+/g, ' '), titleIn(text), ...aliasesOf(text)]
		.filter(Boolean)
		.join(' ');
}
