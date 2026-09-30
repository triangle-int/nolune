import { randomInt, randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { hashPassword } from 'better-auth/crypto';
import { and, eq, ne, sql } from 'drizzle-orm';
import { getDb, type DB } from './db/index.ts';
import { account, user } from './db/schema.ts';
import { inspectImage } from './images.ts';
import { blobPath, storeBytes } from './media.ts';
import { retitleCard, trashCard } from './memory-cards.ts';
import { noticeProfileChanges } from './profiles.ts';

export const MIN_PASSWORD_LENGTH = 14;
export const MAX_NAME_LENGTH = 64;
/** The Settings page sends a 256-pixel square, a small part of this. */
export const MAX_PICTURE_BYTES = 512 * 1024;
export const MAX_PICTURE_EDGE = 1024;

/** Why a name can't be someone's: the web UI says it in its language, the CLI in `message`. */
export class UserNameError extends Error {
	readonly reason: 'required' | 'tooLong' | 'email' | 'taken';
	constructor(message: string, reason: UserNameError['reason']) {
		super(message);
		this.reason = reason;
	}
}

export class EmailError extends Error {
	readonly reason: 'invalid' | 'taken';
	constructor(message: string, reason: EmailError['reason']) {
		super(message);
		this.reason = reason;
	}
}

export type PasswordReason = 'tooShort' | 'tooSimple' | 'tooRepetitive';

export class PasswordError extends Error {
	readonly reason: PasswordReason;
	constructor(message: string, reason: PasswordReason) {
		super(message);
		this.reason = reason;
	}
}

export class PictureError extends Error {
	readonly reason: 'notPicture' | 'tooLarge';
	constructor(message: string, reason: PictureError['reason']) {
		super(message);
		this.reason = reason;
	}
}

/**
 * The name, trimmed, if it can be someone's: names are unique ignoring case, since the CLI and
 * People & profile find people by them, and have no `@`, which findUser takes for an email.
 * `userId`: whose name it's going to be, who may keep their own.
 */
function checkName(name: string, userId?: string): string {
	const trimmed = name.trim();
	if (!trimmed) throw new UserNameError('Name is required', 'required');
	if ([...trimmed].length > MAX_NAME_LENGTH) {
		throw new UserNameError(`Name must be at most ${MAX_NAME_LENGTH} characters`, 'tooLong');
	}
	if (trimmed.includes('@')) throw new UserNameError("Name can't contain @", 'email');
	const taken = getDb()
		.select({ id: user.id })
		.from(user)
		.where(
			and(sql`lower(${user.name}) = lower(${trimmed})`, userId ? ne(user.id, userId) : undefined)
		)
		.get();
	if (taken) throw new UserNameError(`A user named "${trimmed}" already exists`, 'taken');
	return trimmed;
}

const PASSWORD_PROBLEMS: Record<PasswordReason, string> = {
	tooShort: `must be at least ${MIN_PASSWORD_LENGTH} characters`,
	tooSimple: 'needs 3 of: lowercase, uppercase, digits, symbols (or use 20+ characters)',
	tooRepetitive: 'has too few distinct characters'
};

/** Why a password is too weak, or null if it's fine. */
export function passwordReason(password: string): PasswordReason | null {
	if (password.length < MIN_PASSWORD_LENGTH) return 'tooShort';
	const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((re) => re.test(password));
	// Long passphrases are fine without symbol soup.
	if (password.length < 20 && classes.length < 3) return 'tooSimple';
	if (new Set(password).size < 8) return 'tooRepetitive';
	return null;
}

/** Returns why a password is too weak, or null if it's fine. */
export function passwordProblem(password: string): string | null {
	const reason = passwordReason(password);
	return reason && PASSWORD_PROBLEMS[reason];
}

function checkPassword(password: string): void {
	const reason = passwordReason(password);
	if (reason) throw new PasswordError(`Password ${PASSWORD_PROBLEMS[reason]}`, reason);
}

/** The email, trimmed and lowercased, if it's an address nobody has yet. */
function checkEmail(email: string): string {
	const trimmed = email.trim().toLowerCase();
	// findUser takes anything with an @ for an email, and anything without for a name.
	if (!/^[^\s@]+@[^\s@]+$/.test(trimmed)) {
		throw new EmailError(`"${trimmed}" isn't an email address`, 'invalid');
	}
	if (findUserByEmail(trimmed)) {
		throw new EmailError(`A user with email ${trimmed} already exists`, 'taken');
	}
	return trimmed;
}

export function generatePassword(): string {
	const alphabet = 'abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789';
	let out = '';
	for (let i = 0; i < 24; i++) {
		if (i > 0 && i % 6 === 0) out += '-';
		out += alphabet[randomInt(alphabet.length)];
	}
	return out;
}

export function findUserByName(name: string) {
	return getDb()
		.select()
		.from(user)
		.where(sql`lower(${user.name}) = lower(${name})`)
		.get();
}

export function findUserByEmail(email: string) {
	return getDb().select().from(user).where(eq(user.email, email.toLowerCase())).get();
}

export function findUserById(id: string) {
	return getDb().select().from(user).where(eq(user.id, id)).get();
}

/** Accepts either a display name or an email. */
export function findUser(nameOrEmail: string) {
	return nameOrEmail.includes('@') ? findUserByEmail(nameOrEmail) : findUserByName(nameOrEmail);
}

/** What a transaction's callback is handed. */
type Transaction = Parameters<Parameters<DB['transaction']>[0]>[0];

/**
 * `also` runs in the same transaction, before the account is written, and stops it by throwing:
 * acceptInvite uses up its invite there, so a link makes one account however often it's sent.
 */
export async function createUser(
	input: {
		name: string;
		email: string;
		password: string;
		isAdmin?: boolean;
	},
	also?: (tx: Transaction) => void
): Promise<{ id: string }> {
	const name = checkName(input.name);
	const email = checkEmail(input.email);
	checkPassword(input.password);

	const id = randomUUID();
	const hash = await hashPassword(input.password);
	const now = new Date();
	getDb().transaction((tx) => {
		also?.(tx);
		tx.insert(user)
			.values({ id, name, email, isAdmin: input.isAdmin ?? false, createdAt: now, updatedAt: now })
			.run();
		// Same shape better-auth writes for email/password sign-ups.
		tx.insert(account)
			.values({
				id: randomUUID(),
				accountId: id,
				providerId: 'credential',
				userId: id,
				password: hash,
				createdAt: now,
				updatedAt: now
			})
			.run();
	});
	return { id };
}

export async function setPassword(nameOrEmail: string, password: string): Promise<void> {
	const found = findUser(nameOrEmail);
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	checkPassword(password);
	const hash = await hashPassword(password);
	const result = getDb()
		.update(account)
		.set({ password: hash, updatedAt: new Date() })
		.where(and(eq(account.userId, found.id), eq(account.providerId, 'credential')))
		.run();
	if (result.changes === 0) throw new Error('User has no password account');
}

/**
 * What everyone sees them as from now on, and what nolune reads before what they write. Messages
 * already sent keep the name they were sent with.
 */
export function renameUser(userId: string, name: string): string {
	const trimmed = checkName(name, userId);
	getDb().update(user).set({ name: trimmed }).where(eq(user.id, userId)).run();
	// Their card is titled with their name.
	retitleCard(userId, trimmed);
	// Members' names show on the profiles' pages.
	noticeProfileChanges();
	return trimmed;
}

/**
 * Keeps a picture (PNG, JPEG, GIF or WebP) in the media store as their profile picture, in place
 * of their initial. Returns its SHA-256; the old one goes with the next prune.
 */
export function setUserPicture(userId: string, data: Buffer): string {
	const tooLarge = `The picture must be at most ${MAX_PICTURE_EDGE}×${MAX_PICTURE_EDGE} pixels and ${MAX_PICTURE_BYTES / 1024} KB`;
	if (data.length > MAX_PICTURE_BYTES) throw new PictureError(tooLarge, 'tooLarge');
	const info = inspectImage(data);
	if (!info) throw new PictureError('Not a PNG, JPEG, GIF or WebP picture', 'notPicture');
	if (info.width > MAX_PICTURE_EDGE || info.height > MAX_PICTURE_EDGE) {
		throw new PictureError(tooLarge, 'tooLarge');
	}
	const { sha256 } = storeBytes(data);
	getDb().update(user).set({ picture: sha256 }).where(eq(user.id, userId)).run();
	noticeProfileChanges();
	return sha256;
}

/** Back to their initial. */
export function clearUserPicture(userId: string): void {
	getDb().update(user).set({ picture: null }).where(eq(user.id, userId)).run();
	noticeProfileChanges();
}

/**
 * The stored picture with this SHA-256, while it's someone's profile picture. Anything else in the
 * media store (chats' pictures and files) is only reachable through its chat.
 */
export function userPictureFile(sha256: string): { path: string; mime: string } | null {
	if (!/^[0-9a-f]{64}$/.test(sha256)) return null;
	const found = getDb().select({ id: user.id }).from(user).where(eq(user.picture, sha256)).get();
	if (!found) return null;
	const path = blobPath(sha256);
	try {
		const info = inspectImage(readFileSync(path));
		return info && { path, mime: info.mediaType };
	} catch {
		return null;
	}
}

export function setAdmin(nameOrEmail: string, isAdmin: boolean): void {
	const found = findUser(nameOrEmail);
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	getDb().update(user).set({ isAdmin }).where(eq(user.id, found.id)).run();
}

export function listUsers() {
	return getDb()
		.select({
			id: user.id,
			name: user.name,
			email: user.email,
			isAdmin: user.isAdmin,
			picture: user.picture
		})
		.from(user)
		.orderBy(user.name)
		.all();
}

/** Deletes the account; their card goes to the trash, like a deleted profile. */
export function deleteUser(nameOrEmail: string): void {
	const found = findUser(nameOrEmail);
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	trashCard(found.id);
	getDb().delete(user).where(eq(user.id, found.id)).run();
	// They're no longer among their profiles' members.
	noticeProfileChanges();
}
