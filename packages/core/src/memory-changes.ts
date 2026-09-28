import { and, asc, desc, eq, gte, isNull } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { conversation, memoryChange } from './db/schema.ts';
import { parseFacts } from './memory-facts.ts';
import { MemoryError, revertMemoryLines } from './memory.ts';

/*
 * What the note-taker (memory-learning.ts) changed in memory, and the chat it learned it from, so
 * people see it: in that chat, after the last message it read, and on the Memory page. Each change
 * can be undone from either, which puts back the lines it left in the note, as long as nobody
 * changed them since. The agent's own saves show as its commands, and people's edits are their
 * own, so neither is kept here.
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
}): void {
	if (!input.changes.length) return;
	getDb()
		.insert(memoryChange)
		.values(
			input.changes.map((change) => ({
				profileId: input.profileId,
				conversationId: input.conversationId,
				afterMessageId: input.afterMessageId,
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

function toDisplay(row: Row): DisplayMemoryChange {
	return {
		id: row.id,
		op: row.op,
		note: row.note,
		fact: plainLines(row.line),
		before: row.before === null ? null : plainLines(row.before),
		createdAt: row.createdAt.getTime(),
		undone: row.undoneAt ? { at: row.undoneAt.getTime(), by: row.undoneBy } : null
	};
}

/** What the note-taker saved from a chat, each look with the message it shows after. */
export function memoryLooks(conversationId: string): DisplayMemoryLook[] {
	const rows = getDb()
		.select()
		.from(memoryChange)
		.where(eq(memoryChange.conversationId, conversationId))
		.orderBy(asc(memoryChange.id))
		.all();
	const looks = new Map<number, DisplayMemoryLook>();
	for (const row of rows) {
		let look = looks.get(row.afterMessageId);
		if (!look) {
			look = { after: row.afterMessageId, createdAt: row.createdAt.getTime(), changes: [] };
			looks.set(row.afterMessageId, look);
		}
		look.changes.push(toDisplay(row));
	}
	return [...looks.values()];
}

/** A change on the Memory page, with the chat it came from (null once that was deleted). */
export interface RecentMemoryChange extends DisplayMemoryChange {
	conversation: { id: string; title: string } | null;
}

/** What the note-taker saved in a profile lately, newest first. */
export function recentMemoryChanges(
	profileId: string,
	{ since, limit }: { since: Date; limit: number }
): RecentMemoryChange[] {
	return getDb()
		.select({ change: memoryChange, title: conversation.title })
		.from(memoryChange)
		.leftJoin(conversation, eq(conversation.id, memoryChange.conversationId))
		.where(and(eq(memoryChange.profileId, profileId), gte(memoryChange.createdAt, since)))
		.orderBy(desc(memoryChange.id))
		.limit(limit)
		.all()
		.map(({ change, title }) => ({
			...toDisplay(change),
			conversation: change.conversationId ? { id: change.conversationId, title: title ?? '' } : null
		}));
}

/** Why a change can't be undone: it already was, or its lines changed since. */
export class MemoryUndoError extends Error {
	readonly reason: 'undone' | 'changed' | 'missing';

	constructor(reason: MemoryUndoError['reason'], message: string) {
		super(message);
		this.reason = reason;
	}
}

/**
 * Undoes a change of the profile's: an added fact goes (and a note it started, once empty), a
 * replaced one reads as before. Returns the chat it came from, whose open views need to know.
 */
export function undoMemoryChange(
	profile: { id: string; slug: string },
	id: number,
	by: string
): { conversationId: string | null } {
	const db = getDb();
	const row = db
		.select()
		.from(memoryChange)
		.where(and(eq(memoryChange.id, id), eq(memoryChange.profileId, profile.id)))
		.get();
	if (!row) throw new MemoryUndoError('missing', 'No such memory change.');
	if (row.undoneAt) throw new MemoryUndoError('undone', 'It was already undone.');
	try {
		if (row.op === 'add')
			revertMemoryLines(profile.slug, row.note, row.line, null, row.createdNote);
		else revertMemoryLines(profile.slug, row.note, row.line, row.before);
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
