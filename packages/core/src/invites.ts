import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { and, desc, eq, gt, isNull, lte, or } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { invite, user } from './db/schema.ts';
import { MAX_NAME_LENGTH, createUser } from './users.ts';

/*
 * Invite links: an admin makes one on the People page (or `nolune user invite`) and sends it, and
 * whoever opens it picks their own name, email and password. So nobody has to pass a password
 * along, and nolune never shows one to the agent. A link makes one ordinary account, for a week,
 * while the admin who made it still is one; the account can be made an admin afterwards.
 */

export const INVITE_DAYS = 7;

export class InviteError extends Error {
	constructor() {
		super('This invite link has been used, has expired or was taken back');
	}
}

export interface Invite {
	id: string;
	/** Who it's for, as the admin wrote it. */
	name: string | null;
	/** The name of the admin who made it; null when it was made at this computer. */
	createdBy: string | null;
	createdAt: Date;
	expiresAt: Date;
}

function hashToken(token: string): string {
	return createHash('sha256').update(token).digest('hex');
}

/** Unexpired, and made by an admin or at this computer. */
function usable() {
	return and(
		gt(invite.expiresAt, new Date()),
		or(isNull(invite.createdBy), eq(user.isAdmin, true))
	);
}

function selectInvites() {
	return getDb()
		.select({
			id: invite.id,
			name: invite.name,
			createdBy: user.name,
			createdAt: invite.createdAt,
			expiresAt: invite.expiresAt
		})
		.from(invite)
		.leftJoin(user, eq(invite.createdBy, user.id));
}

/** The invite and its token, which goes in the link and isn't kept. */
export function createInvite(input: { name?: string | null; createdBy?: string | null }): {
	invite: Invite;
	token: string;
} {
	const db = getDb();
	// Expired ones are no use to anyone.
	db.delete(invite).where(lte(invite.expiresAt, new Date())).run();
	const id = randomUUID();
	const token = randomBytes(24).toString('base64url');
	const createdAt = new Date();
	db.insert(invite)
		.values({
			id,
			tokenHash: hashToken(token),
			name: [...(input.name?.trim() ?? '')].slice(0, MAX_NAME_LENGTH).join('') || null,
			createdBy: input.createdBy ?? null,
			createdAt,
			expiresAt: new Date(createdAt.getTime() + INVITE_DAYS * 24 * 60 * 60 * 1000)
		})
		.run();
	return { invite: selectInvites().where(eq(invite.id, id)).get()!, token };
}

/** The invites that still work, newest first, with the name of whoever made each. */
export function listInvites(): Invite[] {
	return selectInvites().where(usable()).orderBy(desc(invite.createdAt)).all();
}

/** Takes an invite back: its link stops working. */
export function revokeInvite(id: string): void {
	getDb().delete(invite).where(eq(invite.id, id)).run();
}

/** The invite a link's token is for, while it works. */
export function findInvite(token: string): Invite | null {
	return (
		selectInvites()
			.where(and(eq(invite.tokenHash, hashToken(token)), usable()))
			.get() ?? null
	);
}

/**
 * Makes the account an invite is for and uses the invite up, together: of two people sending the
 * same link at once, one gets an account and the other an InviteError. Throws what createUser
 * throws for a name, email or password it can't take, and the invite keeps working.
 */
export async function acceptInvite(
	token: string,
	input: { name: string; email: string; password: string }
): Promise<{ id: string }> {
	const found = findInvite(token);
	if (!found) throw new InviteError();
	return createUser(input, (tx) => {
		// Still there after the password was hashed, and still made by an admin.
		const still = tx
			.select({ id: invite.id })
			.from(invite)
			.leftJoin(user, eq(invite.createdBy, user.id))
			.where(and(eq(invite.id, found.id), usable()))
			.get();
		if (!still) throw new InviteError();
		tx.delete(invite).where(eq(invite.id, found.id)).run();
	});
}
