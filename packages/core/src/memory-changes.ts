import { and, asc, desc, eq, gte, inArray, isNull } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { card, conversation, memoryChange, user } from './db/schema.ts';
import { CARDS_FOLDER, isCardPath } from './memory-categories.ts';
import { parseFacts } from './memory-facts.ts';
import { MemoryError, revertMemoryLines } from './memory.ts';
import { CARDS } from './paths.ts';

/*
 * What the note-taker (memory-learning.ts) changed in memory, and the chat it learned it from, so
 * people see it: in that chat, after the last message it read, and on the Memory page. Each change
 * can be undone from either, which puts back the lines it left in the note, as long as nobody
 * changed them since. The agent's own saves show as its commands, and people's edits are their
 * own, so neither is kept here, except the agent's on a card (memory-cards.ts): a card's owner
 * sees every change to it, from any of their profiles, and only they can undo one.
 */

/** A change as the note-taker made it, with the lines it left in the note. */
export interface RecordedMemoryChange {
	op: 'add' | 'replace';
	/** The note's path, like `people/leo.md`. */
	note: string;
	/** The lines as they are in the note now. */
	line: string;
	/** A replace: the lines as they were. */
	before?: string;
	/** An add that started the note. */
	createdNote?: boolean;
}

export function recordMemoryChanges(input: {
	profileId: string;
	conversationId: string;
	/** The last message it read. */
	afterMessageId: number;
	changes: RecordedMemoryChange[];
	/** The note-taker's (the default), or the agent's on a card. */
	source?: 'learning' | 'agent';
}): void {
	if (!input.changes.length) return;
	getDb()
		.insert(memoryChange)
		.values(
			input.changes.map((change) => ({
				profileId: input.profileId,
				conversationId: input.conversationId,
				afterMessageId: input.afterMessageId,
				source: input.source ?? 'learning',
				op: change.op,
				note: change.note,
				line: change.line,
				before: change.before ?? null,
				createdNote: change.createdNote ?? false
			}))
		)
		.run();
}

/** A change as people see it. */
export interface DisplayMemoryChange {
	id: number;
	op: 'add' | 'replace';
	/** The note's path, like `people/leo.md`. */
	note: string;
	/** The fact as the change left it, as plain text. */
	fact: string;
	/** A replace: what it said before. */
	before: string | null;
	createdAt: number;
	/** Undone, when and by whom. */
	undone: { at: number; by: string | null } | null;
	/** A change to a card: whose it is. Only they can undo it. */
	card: { ownerId: string; owner: string } | null;
}

/** What one look at a chat changed, shown after the last message it read. */
export interface DisplayMemoryLook {
	/** The message it shows after. */
	after: number;
	createdAt: number;
	changes: DisplayMemoryChange[];
}

type Row = typeof memoryChange.$inferSelect;

/** Lines of a note as plain text: its facts, or the lines themselves when they're not facts. */
function plainLines(lines: string): string {
	return parseFacts(lines).join(' · ') || lines.trim();
}

/** The owners of the cards among `notes`, by path. */
function cardOwners(notes: string[]): Map<string, { ownerId: string; owner: string }> {
	const names = [...new Set(notes.filter(isCardPath))].map((note) =>
		note.replace(/\.(md|markdown|txt)$/i, '').slice(CARDS_FOLDER.length + 1)
	);
	if (!names.length) return new Map();
	return new Map(
		getDb()
			.select({ name: card.name, ownerId: user.id, owner: user.name })
			.from(card)
			.innerJoin(user, eq(user.id, card.userId))
			.where(inArray(card.name, names))
			.all()
			.map((row) => [`${CARDS_FOLDER}/${row.name}.md`, { ownerId: row.ownerId, owner: row.owner }])
	);
}

function toDisplay(
	row: Row,
	owners: Map<string, { ownerId: string; owner: string }>
): DisplayMemoryChange {
	return {
		id: row.id,
		op: row.op,
		note: row.note,
		fact: plainLines(row.line),
		before: row.before === null ? null : plainLines(row.before),
		createdAt: row.createdAt.getTime(),
		undone: row.undoneAt ? { at: row.undoneAt.getTime(), by: row.undoneBy } : null,
		card: owners.get(row.note) ?? null
	};
}

/**
 * What the note-taker saved from a chat, each look with the message it shows after. The agent's
 * changes to a card show in the chat as its commands.
 */
export function memoryLooks(conversationId: string): DisplayMemoryLook[] {
	const rows = getDb()
		.select()
		.from(memoryChange)
		.where(
			and(eq(memoryChange.conversationId, conversationId), eq(memoryChange.source, 'learning'))
		)
		.orderBy(asc(memoryChange.id))
		.all();
	const owners = cardOwners(rows.map((row) => row.note));
	const looks = new Map<number, DisplayMemoryLook>();
	for (const row of rows) {
		let look = looks.get(row.afterMessageId);
		if (!look) {
			look = { after: row.afterMessageId, createdAt: row.createdAt.getTime(), changes: [] };
			looks.set(row.afterMessageId, look);
		}
		look.changes.push(toDisplay(row, owners));
	}
	return [...looks.values()];
}

/** A change on the Memory page, with the chat it came from (null once that was deleted). */
export interface RecentMemoryChange extends DisplayMemoryChange {
	conversation: { id: string; title: string } | null;
}

/**
 * What the note-taker saved from a profile's chats lately, newest first: on its members' cards
 * too, but only from this profile's chats, so nobody learns where a card fact came from unless
 * they're in that profile.
 */
export function recentMemoryChanges(
	profileId: string,
	{ since, limit }: { since: Date; limit: number }
): RecentMemoryChange[] {
	const rows = getDb()
		.select({ change: memoryChange, title: conversation.title })
		.from(memoryChange)
		.leftJoin(conversation, eq(conversation.id, memoryChange.conversationId))
		.where(
			and(
				eq(memoryChange.profileId, profileId),
				eq(memoryChange.source, 'learning'),
				gte(memoryChange.createdAt, since)
			)
		)
		.orderBy(desc(memoryChange.id))
		.limit(limit)
		.all();
	const owners = cardOwners(rows.map(({ change }) => change.note));
	return rows.map(({ change, title }) => ({
		...toDisplay(change, owners),
		conversation: change.conversationId ? { id: change.conversationId, title: title ?? '' } : null
	}));
}

/**
 * Why a change can't be undone: it already was, its lines changed since, or it's on someone
 * else's card.
 */
export class MemoryUndoError extends Error {
	readonly reason: 'undone' | 'changed' | 'missing' | 'owner';

	constructor(reason: MemoryUndoError['reason'], message: string) {
		super(message);
		this.reason = reason;
	}
}

/**
 * Undoes a change of the profile's: an added fact goes (and a note it started, once empty), a
 * replaced one reads as before. A change to a card only by its owner (`userId`). Returns the chat
 * it came from, whose open views need to know.
 */
export function undoMemoryChange(
	profile: { id: string; slug: string },
	id: number,
	by: string,
	userId?: string
): { conversationId: string | null } {
	const db = getDb();
	const row = db
		.select()
		.from(memoryChange)
		.where(and(eq(memoryChange.id, id), eq(memoryChange.profileId, profile.id)))
		.get();
	if (!row) throw new MemoryUndoError('missing', 'No such memory change.');
	if (row.undoneAt) throw new MemoryUndoError('undone', 'It was already undone.');
	const onCard = isCardPath(row.note);
	if (onCard && cardOwners([row.note]).get(row.note)?.ownerId !== userId) {
		throw new MemoryUndoError('owner', 'Only its owner can change a card.');
	}
	const place = onCard ? CARDS : profile.slug;
	try {
		if (row.op === 'add') revertMemoryLines(place, row.note, row.line, null, row.createdNote);
		else revertMemoryLines(place, row.note, row.line, row.before);
	} catch (err) {
		if (err instanceof MemoryError) throw new MemoryUndoError('changed', err.message);
		throw err;
	}
	db.update(memoryChange)
		.set({ undoneAt: new Date(), undoneBy: by })
		.where(and(eq(memoryChange.id, id), isNull(memoryChange.undoneAt)))
		.run();
	return { conversationId: row.conversationId };
}
