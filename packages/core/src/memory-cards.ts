import { existsSync, mkdirSync, renameSync } from 'node:fs';
import { join } from 'node:path';
import { and, asc, desc, eq } from 'drizzle-orm';
import { committedRows, lastCommittedRow, rowCalls } from './conversations.ts';
import { getDb } from './db/index.ts';
import {
	card as cardTable,
	conversation,
	memoryChange,
	profile as profileTable,
	profileMember,
	user
} from './db/schema.ts';
import { CARDS_FOLDER, isAliasLine, isCardPath, noteName, whoIn } from './memory-categories.ts';
import { recordMemoryChanges, type RecordedMemoryChange } from './memory-changes.ts';
import { membersWithNotes } from './memory-people.ts';
import { factKey, readFacts } from './memory-facts.ts';
import {
	CORE_NOTE,
	MAX_CARD_CHARS,
	MemoryError,
	addMemoryFact,
	addMemoryFacts,
	factLines,
	forgetMemoryFact,
	forgetMemoryFile,
	listMemoryFiles,
	readMemoryNote,
	readPinnedNote,
	replaceInMemory,
	revertMemoryLines,
	writeMemoryFile,
	writeMemoryNote,
	type MemoryFact,
	type MemoryFile
} from './memory.ts';
import { CARDS, paths } from './paths.ts';

/*
 * Cards: a note per user that goes with them into every profile they're a member of, so they
 * don't tell each one who they are again. Profiles are how people keep their circles apart (a
 * surprise is planned in a profile without the person it's for), so nothing moves from one
 * profile to another by itself: only what someone says about themselves, and would tell any of
 * their circles, goes on their card. Everyone in any of those profiles can read it; only its
 * owner's own words write it. Everything else stays in the profile it was said in.
 *
 * A card is `~/.nolune/cards/<name>.md`, kept by memory.ts like a profile's notes (CARDS in place
 * of a slug), and reads as `cards/<name>.md` in every profile its owner is in. It is pinned: it
 * goes whole into the prompts, so it holds at most MAX_CARD_CHARS.
 */

export interface Card {
	userId: string;
	/** Its owner's display name. */
	owner: string;
	/** Like `anna`, fixed once picked. */
	name: string;
	/** How memory calls it, in every profile: `cards/anna.md`. */
	path: string;
}

type ProfileRef = { id: string; slug: string };

function refuse(message: string): never {
	throw new MemoryError(message);
}

function toCard(row: { userId: string; owner: string; name: string }): Card {
	return { ...row, path: `${CARDS_FOLDER}/${row.name}.md` };
}

/** A name for a new card: the first name, else the whole name, else the first with a number. */
function freshName(userName: string): string {
	const taken = new Set(
		getDb()
			.select({ name: cardTable.name })
			.from(cardTable)
			.all()
			.map((row) => row.name)
	);
	const free = (name: string) => !taken.has(name) && !existsSync(join(paths.cards, `${name}.md`));
	const first = noteName(userName.trim().split(/\s+/)[0] ?? userName);
	for (const name of [first, noteName(userName)]) if (free(name)) return name;
	for (let i = 2; ; i++) if (free(`${first}-${i}`)) return `${first}-${i}`;
}

/** A user's card, named the first time it's needed. Throws for a user that doesn't exist. */
export function cardOf(userId: string): Card {
	const db = getDb();
	const row = () =>
		db
			.select({ userId: cardTable.userId, owner: user.name, name: cardTable.name })
			.from(cardTable)
			.innerJoin(user, eq(user.id, cardTable.userId))
			.where(eq(cardTable.userId, userId))
			.get();
	const found = row();
	if (found) return toCard(found);
	const owner = db.select({ name: user.name }).from(user).where(eq(user.id, userId)).get();
	if (!owner) throw new Error('No such user');
	db.insert(cardTable)
		.values({ userId, name: freshName(owner.name) })
		.onConflictDoNothing({ target: cardTable.userId })
		.run();
	return toCard(row()!);
}

/** The cards of a profile's members, by their owners' names. */
export function profileCards(profileId: string): Card[] {
	const members = getDb()
		.select({ id: user.id })
		.from(profileMember)
		.innerJoin(user, eq(user.id, profileMember.userId))
		.where(eq(profileMember.profileId, profileId))
		.orderBy(asc(user.name))
		.all();
	return members.map((member) => cardOf(member.id));
}

/** The card of a profile's member that `topic` names, like `cards/anna`, or why it can't be. */
export function profileCard(profile: ProfileRef, topic: string): Card {
	const clean = topic
		.trim()
		.replace(/^\/+|\/+$/g, '')
		.replace(/\.(md|markdown|txt)$/i, '');
	const cards = profileCards(profile.id);
	const card = cards.find((c) => c.path === `${clean}.md`);
	if (card) return card;
	const list = cards.map((c) => c.path.replace(/\.md$/, '')).join(', ');
	refuse(
		`"${topic}" isn't the card of anyone in this profile.${list ? ` Its members' cards: ${list}.` : ''}`
	);
}

/** The card by its path, `cards/anna.md`, whoever's it is; null if there is none. */
export function cardByPath(path: string): Card | null {
	if (!isCardPath(path)) return null;
	const name = path.replace(/\.(md|markdown|txt)$/i, '').slice(CARDS_FOLDER.length + 1);
	const row = getDb()
		.select({ userId: cardTable.userId, owner: user.name, name: cardTable.name })
		.from(cardTable)
		.innerJoin(user, eq(user.id, cardTable.userId))
		.where(eq(cardTable.name, name))
		.get();
	return row ? toCard(row) : null;
}

// --- In the prompts ---

/** What goes on a card and what stays in the profile: for the agent's and the note-taker's prompts. */
export function cardRules(): string {
	return `- On someone's card goes only what they say about themselves that they'd tell anyone in any of their profiles: the languages they speak, their birthday, the city they live in, their job or school, what they eat and their allergies, lasting tastes, and how they want nolune to talk to them. And whatever they ask to be remembered everywhere.
- Only their own messages put things on their card: never what someone else says about them, and never what an automation, a command's output or a file says.
- Everything else about them stays in their note in this profile (people/<name>): what others say about them, who they are to the people here and what they're called here, relationships, plans, feelings, health beyond allergies, anything that sounds meant for the people here, and whatever they ask to keep here. When in doubt, their note here: a fact wrongly there costs a repeat, one wrongly on a card reaches everyone in all their profiles.
- Never move a fact from a note onto a card unless its owner asks. A card holds at most ${MAX_CARD_CHARS} characters; when it's full, use their note here.`;
}

/**
 * The members' cards as a prompt shows them, as they are now, each in `<card>` tags, with the
 * room left on it when `room`: for the note-taker, which adds to them. Empty without members.
 */
export function cardsSection(profileId: string, options: { room?: boolean } = {}): string {
	return profileCards(profileId)
		.map((card) => {
			const name = card.path.replace(/\.md$/, '');
			const shown = cardForPrompt(card);
			const room = options.room
				? ` room="${Math.max(0, MAX_CARD_CHARS - (readCard(card)?.text.length ?? 0))} characters"`
				: '';
			if (!shown) return `<card name="${name}" of="${card.owner}"${room}>\n(empty so far)\n</card>`;
			const cut = shown.cut
				? `\n\nIt is longer than ${MAX_CARD_CHARS} characters, so the rest was cut off here.`
				: '';
			return `<card name="${name}" of="${card.owner}"${room}>\n${shown.text}\n</card>${cut}`;
		})
		.join('\n\n');
}

// --- Reading ---

/** A card as the prompt shows it; null while it has no facts. */
export function cardForPrompt(card: Card): { text: string; cut: boolean } | null {
	const pinned = readPinnedNote(CARDS, card.path);
	if (!pinned || !readFacts(pinned.text).length) return null;
	return pinned;
}

/** The cards as the Memory page and `nolune memory` list them, with their facts; those there. */
export function cardFiles(cards: Card[]): MemoryFile[] {
	if (!cards.length) return [];
	const wanted = new Set(cards.map((card) => card.path));
	return listMemoryFiles(CARDS).filter((file) => wanted.has(file.path));
}

/** A card's note, or null while it isn't there. */
export function readCard(card: Card): { path: string; text: string } | null {
	try {
		return readMemoryNote(CARDS, card.path);
	} catch (err) {
		if (err instanceof MemoryError) return null;
		throw err;
	}
}

/** The profiles a card is in: its owner's. Only for its owner to see. */
export function cardProfiles(userId: string): { id: string; slug: string; name: string }[] {
	return getDb()
		.select({ id: profileTable.id, slug: profileTable.slug, name: profileTable.name })
		.from(profileMember)
		.innerJoin(profileTable, eq(profileTable.id, profileMember.profileId))
		.where(eq(profileMember.userId, userId))
		.orderBy(asc(profileTable.name))
		.all();
}

// --- Writing ---

/** The card's note, titled with its owner's name, started when there is something to write. */
function startCard(card: Card): void {
	if (readCard(card)) return;
	writeMemoryNote(CARDS, card.path, `# ${card.owner}\n`);
}

/** Adds a fact to a card, like `nolune memory add`. */
export function addToCard(card: Card, fact: string, under?: string) {
	startCard(card);
	return addMemoryFact(CARDS, card.path, fact, under);
}

/** Adds facts learned elsewhere (an import, or the notes Bring in takes), with their dates. */
export function addFactsToCard(
	card: Card,
	facts: { text: string; learnedAt: number | null }[],
	heading?: string
): { added: MemoryFact[]; left: MemoryFact[] } {
	if (!facts.length) return { added: [], left: [] };
	startCard(card);
	const { added, left } = addMemoryFacts(CARDS, card.path, facts, heading);
	return { added, left };
}

export function replaceInCard(card: Card, oldText: string, newText: string) {
	return replaceInMemory(CARDS, card.path, oldText, newText);
}

export function forgetInCard(card: Card, match: string) {
	return forgetMemoryFact(CARDS, card.path, match);
}

/** Saves a card its owner edited on its page; see writeMemoryFile for `basedOn`. */
export function writeCardFile(card: Card, text: string, basedOn: number): void {
	writeMemoryFile(CARDS, card.path, text, basedOn);
}

/** Empties a card, when its owner deletes it on its page. */
export function forgetCardFile(card: Card): void {
	forgetMemoryFile(CARDS, card.path);
}

/**
 * Refuses a change to `card` by the agent of a chat unless the card's owner wrote one of the
 * messages it's answering: those since its last final reply. So automation runs and subagents,
 * whose chats have nobody's messages, never write a card, and neither does what a command read.
 * What a person writes on the card's page is theirs to write, and doesn't come through here.
 */
export function checkCardWrite(
	card: Card,
	chat: { profileId: string; conversationId: string | undefined }
): void {
	const why = `${card.path.replace(/\.md$/, '')} is ${card.owner}'s card, which goes with them into all their profiles: only what ${card.owner} says about themselves goes on it, from their own messages.`;
	if (!chat.conversationId) refuse(`${why} ${card.owner} can also edit it on its page.`);
	const conv = getDb()
		.select({ profileId: conversation.profileId })
		.from(conversation)
		.where(eq(conversation.id, chat.conversationId))
		.get();
	if (!conv || conv.profileId !== chat.profileId) refuse(why);
	const rows = committedRows(chat.conversationId);
	let start = 0;
	for (let i = rows.length - 1; i >= 0; i--) {
		if (rows[i].kind === 'assistant' && !rowCalls(rows[i]).length) {
			start = i + 1;
			break;
		}
	}
	if (!rows.slice(start).some((row) => row.kind === 'human' && row.senderId === card.userId)) {
		refuse(
			`${why} Save what you learned in this profile instead (their note in people/), or ask ${card.owner} to tell you themselves.`
		);
	}
}

/**
 * Records what the agent of a chat changed on a card, so its owner sees it on the card's page,
 * with Undo and Keep only here. The chat shows it as the agent's command.
 */
export function recordAgentCardChanges(
	chat: { profileId: string; conversationId: string },
	changes: RecordedMemoryChange[]
): void {
	if (!changes.length) return;
	recordMemoryChanges({
		profileId: chat.profileId,
		conversationId: chat.conversationId,
		afterMessageId: lastCommittedRow(chat.conversationId)?.id ?? 0,
		changes,
		source: 'agent'
	});
}

/**
 * A card is titled with its owner's name, which they can change in Settings: its `# title`
 * follows, so search and the note-taker know it by the name they go by now.
 */
export function retitleCard(userId: string, name: string): void {
	const row = getDb()
		.select({ name: cardTable.name })
		.from(cardTable)
		.where(eq(cardTable.userId, userId))
		.get();
	if (!row) return;
	const card = toCard({ userId, owner: name, name: row.name });
	const note = readCard(card);
	if (!note) return;
	const lines = note.text.split('\n');
	const first = lines.findIndex((line) => line.trim());
	if (first !== -1 && /^#\s/.test(lines[first].trim())) lines[first] = `# ${name}`;
	else lines.unshift(`# ${name}`, '');
	const text = lines.join('\n');
	if (text !== note.text) writeMemoryNote(CARDS, card.path, text);
}

/** Moves a deleted user's card to the trash, like a deleted profile's folder. */
export function trashCard(userId: string): void {
	const row = getDb()
		.select({ name: cardTable.name })
		.from(cardTable)
		.where(eq(cardTable.userId, userId))
		.get();
	if (!row) return;
	const file = join(paths.cards, `${row.name}.md`);
	if (!existsSync(file)) return;
	mkdirSync(paths.trash, { recursive: true });
	renameSync(file, join(paths.trash, `card-${row.name}-${Date.now()}.md`));
}

// --- What changed on a card ---

export interface CardChange {
	id: number;
	op: 'add' | 'replace';
	source: 'learning' | 'agent';
	/** The fact as the change left it, as plain text. */
	fact: string;
	before: string | null;
	createdAt: number;
	undone: { at: number; by: string | null } | null;
	/** Where it came from: only its owner sees this. */
	profile: { slug: string; name: string };
	conversation: { id: string; title: string } | null;
}

function plain(lines: string): string {
	return (
		readFacts(lines)
			.map((fact) => fact.text)
			.join(' · ') || lines.trim()
	);
}

/** Every change the agent and the note-taker made to a card, from any profile, newest first. */
export function cardChanges(card: Card, limit = 50): CardChange[] {
	return getDb()
		.select({
			change: memoryChange,
			slug: profileTable.slug,
			profileName: profileTable.name,
			title: conversation.title
		})
		.from(memoryChange)
		.innerJoin(profileTable, eq(profileTable.id, memoryChange.profileId))
		.leftJoin(conversation, eq(conversation.id, memoryChange.conversationId))
		.where(eq(memoryChange.note, card.path))
		.orderBy(desc(memoryChange.id))
		.limit(limit)
		.all()
		.map(({ change, slug, profileName, title }) => ({
			id: change.id,
			op: change.op,
			source: change.source,
			fact: plain(change.line),
			before: change.before === null ? null : plain(change.before),
			createdAt: change.createdAt.getTime(),
			undone: change.undoneAt ? { at: change.undoneAt.getTime(), by: change.undoneBy } : null,
			profile: { slug, name: profileName },
			conversation: change.conversationId ? { id: change.conversationId, title: title ?? '' } : null
		}));
}

/**
 * Moves what a change put on its owner's card into their note in the profile it came from, when
 * it belongs to that circle only: the card goes back to how it was, and the note gets the fact.
 */
export function keepOnlyInProfile(
	changeId: number,
	userId: string
): {
	profile: { id: string; slug: string; name: string };
	note: string;
	conversationId: string | null;
} {
	const db = getDb();
	const row = db.select().from(memoryChange).where(eq(memoryChange.id, changeId)).get();
	const card = row ? cardByPath(row.note) : null;
	if (!row || !card || card.userId !== userId) refuse('That is not a change to your card.');
	if (row.undoneAt) refuse('That change was already undone.');
	const target = db
		.select({
			id: profileTable.id,
			slug: profileTable.slug,
			name: profileTable.name,
			note: profileMember.personNote
		})
		.from(profileMember)
		.innerJoin(profileTable, eq(profileTable.id, profileMember.profileId))
		.where(and(eq(profileMember.profileId, row.profileId), eq(profileMember.userId, userId)))
		.get();
	if (!target) refuse("You're no longer in the profile it came from.");
	if (!target.note) {
		refuse(`Which note is yours in ${target.name} isn't known yet: say so in its settings first.`);
	}
	revertMemoryLines(CARDS, card.path, row.line, row.op === 'add' ? null : row.before);
	for (const fact of factLines(row.line)) addMemoryFact(target.slug, target.note, fact);
	db.update(memoryChange)
		.set({ undoneAt: new Date(), undoneBy: card.owner })
		.where(eq(memoryChange.id, changeId))
		.run();
	const { note, ...profile } = target;
	return { profile, note, conversationId: row.conversationId };
}

// --- Starting a card from the notes people have ---

/** A fact about someone in one of their profiles that could go on their card. */
export interface CardCandidate {
	/** Says which one, for bringToCard: the profile, the note and the fact, not where it is. */
	id: string;
	profile: { id: string; slug: string; name: string };
	/** The note it's in: their person note, or core. */
	note: string;
	/** Where it starts in the note, from 1. */
	line: number;
	/** As it would go on the card. */
	text: string;
	heading: string | null;
	learnedAt: number | null;
	/** Checked at first: what two or more of their notes say, and their rules pinned in core. */
	suggested: boolean;
}

/** A line of a profile's core with the person's name in front: `Anna: keep answers short`. */
function ownRule(text: string, owner: string): string | null {
	const first = owner.trim().split(/\s+/)[0];
	for (const name of [owner.trim(), first]) {
		const prefix = `${name}:`;
		if (name && text.toLowerCase().startsWith(prefix.toLowerCase())) {
			const rest = text.slice(prefix.length).trim();
			return rest ? rest.charAt(0).toUpperCase() + rest.slice(1) : null;
		}
	}
	return null;
}

/**
 * What the owner's notes in each of their profiles say about them, to start their card from: the
 * facts of their note there (without who they are to the people in it and what they're called
 * there, which belong to that circle), and their rules in its core. Facts the card has already
 * are left out.
 */
export function cardCandidates(userId: string): CardCandidate[] {
	const card = cardOf(userId);
	const onCard = new Set(readFacts(readCard(card)?.text ?? '').map((f) => factKey(f.text)));
	const found: CardCandidate[] = [];
	// Each member's note is known once someone looked (see membersWithNotes).
	for (const profile of cardProfiles(userId)) membersWithNotes(profile);
	const rows = getDb()
		.select({
			id: profileTable.id,
			slug: profileTable.slug,
			name: profileTable.name,
			note: profileMember.personNote
		})
		.from(profileMember)
		.innerJoin(profileTable, eq(profileTable.id, profileMember.profileId))
		.where(eq(profileMember.userId, userId))
		.orderBy(asc(profileTable.name))
		.all();
	for (const row of rows) {
		const profile = { id: row.id, slug: row.slug, name: row.name };
		const files = new Map(listMemoryFiles(row.slug).map((file) => [file.path, file]));
		const dated = (file: MemoryFile | undefined, text: string) =>
			file?.facts.find((fact) => fact.text === text)?.learnedAt ?? null;
		const person = row.note ? files.get(row.note) : undefined;
		for (const fact of person ? readFacts(person.text) : []) {
			if (isAliasLine(fact.text) || whoIn(fact.text)) continue;
			found.push({
				id: `${row.id}:${row.note}:${factKey(fact.text)}`,
				profile,
				note: row.note!,
				line: fact.line,
				text: fact.text,
				heading: fact.heading,
				learnedAt: dated(person, fact.text),
				suggested: false
			});
		}
		const core = files.get(CORE_NOTE);
		for (const fact of core ? readFacts(core.text) : []) {
			const text = ownRule(fact.text, card.owner);
			if (!text) continue;
			found.push({
				id: `${row.id}:${CORE_NOTE}:${factKey(fact.text)}`,
				profile,
				note: CORE_NOTE,
				line: fact.line,
				text,
				heading: 'Instructions',
				learnedAt: dated(core, fact.text),
				suggested: true
			});
		}
	}
	const inProfiles = new Map<string, Set<string>>();
	for (const c of found) {
		const key = factKey(c.text);
		inProfiles.set(key, (inProfiles.get(key) ?? new Set()).add(c.profile.id));
	}
	return found
		.filter((c) => !onCard.has(factKey(c.text)))
		.map((c) => ({
			...c,
			suggested: c.suggested || (inProfiles.get(factKey(c.text))?.size ?? 0) > 1
		}));
}

/**
 * Moves the chosen facts (cardCandidates' ids) onto the owner's card, with the earliest date any
 * copy had, and takes them out of the notes they came from, which read the card now. What
 * doesn't fit on the card stays where it was.
 */
export function bringToCard(userId: string, ids: string[]): { added: number; left: number } {
	const card = cardOf(userId);
	const chosen = new Set(ids);
	const picked = cardCandidates(userId).filter((c) => chosen.has(c.id));
	if (!picked.length) return { added: 0, left: 0 };
	// One fact from several profiles goes on once, with its earliest date.
	const unique = new Map<string, { text: string; learnedAt: number | null; heading: string }>();
	for (const c of picked) {
		const key = factKey(c.text);
		const known = unique.get(key);
		const learnedAt =
			known?.learnedAt == null
				? c.learnedAt
				: c.learnedAt == null
					? known.learnedAt
					: Math.min(known.learnedAt, c.learnedAt);
		unique.set(key, { text: known?.text ?? c.text, learnedAt, heading: c.heading ?? 'About' });
	}
	const byHeading = new Map<string, { text: string; learnedAt: number | null }[]>();
	for (const fact of unique.values()) {
		byHeading.set(fact.heading, [
			...(byHeading.get(fact.heading) ?? []),
			{ text: fact.text, learnedAt: fact.learnedAt }
		]);
	}
	const onCard = new Set<string>();
	let left = 0;
	for (const [heading, facts] of byHeading) {
		const result = addFactsToCard(card, facts, heading);
		for (const fact of result.added) onCard.add(factKey(fact.text));
		left += result.left.length;
	}
	// The card has them now, in each of those profiles: out of the notes they came from.
	for (const c of picked) {
		if (!onCard.has(factKey(c.text))) continue;
		const file = listMemoryFiles(c.profile.slug).find((f) => f.path === c.note);
		const line = file?.text.split('\n')[c.line - 1];
		if (line === undefined) continue;
		try {
			revertMemoryLines(c.profile.slug, c.note, line, null);
		} catch (err) {
			// Changed meanwhile: it stays, and the card has it too.
			if (!(err instanceof MemoryError)) throw err;
		}
	}
	return { added: onCard.size, left };
}
