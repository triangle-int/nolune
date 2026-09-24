import { randomInt, randomUUID } from 'node:crypto';
import { hashPassword } from 'better-auth/crypto';
import { and, eq, sql } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { account, user } from './db/schema.ts';

export const MIN_PASSWORD_LENGTH = 14;

/** Returns why a password is too weak, or null if it's fine. */
export function passwordProblem(password: string): string | null {
	if (password.length < MIN_PASSWORD_LENGTH) {
		return `must be at least ${MIN_PASSWORD_LENGTH} characters`;
	}
	const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^a-zA-Z0-9]/].filter((re) => re.test(password));
	// Long passphrases are fine without symbol soup.
	if (password.length < 20 && classes.length < 3) {
		return 'needs 3 of: lowercase, uppercase, digits, symbols (or use 20+ characters)';
	}
	if (new Set(password).size < 8) return 'has too few distinct characters';
	return null;
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

/** Accepts either a display name or an email. */
export function findUser(nameOrEmail: string) {
	return nameOrEmail.includes('@') ? findUserByEmail(nameOrEmail) : findUserByName(nameOrEmail);
}

export async function createUser(input: {
	name: string;
	email: string;
	password: string;
	isAdmin?: boolean;
}): Promise<{ id: string }> {
	const name = input.name.trim();
	const email = input.email.trim().toLowerCase();
	if (!name) throw new Error('Name is required');
	if (findUserByName(name)) throw new Error(`A user named "${name}" already exists`);
	if (findUserByEmail(email)) throw new Error(`A user with email ${email} already exists`);
	const problem = passwordProblem(input.password);
	if (problem) throw new Error(`Password ${problem}`);

	const id = randomUUID();
	const hash = await hashPassword(input.password);
	const now = new Date();
	getDb().transaction((tx) => {
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
	const problem = passwordProblem(password);
	if (problem) throw new Error(`Password ${problem}`);
	const hash = await hashPassword(password);
	const result = getDb()
		.update(account)
		.set({ password: hash, updatedAt: new Date() })
		.where(and(eq(account.userId, found.id), eq(account.providerId, 'credential')))
		.run();
	if (result.changes === 0) throw new Error('User has no password account');
}

export function setAdmin(nameOrEmail: string, isAdmin: boolean): void {
	const found = findUser(nameOrEmail);
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	getDb().update(user).set({ isAdmin }).where(eq(user.id, found.id)).run();
}

export function listUsers() {
	return getDb()
		.select({ id: user.id, name: user.name, email: user.email, isAdmin: user.isAdmin })
		.from(user)
		.orderBy(user.name)
		.all();
}

export function deleteUser(nameOrEmail: string): void {
	const found = findUser(nameOrEmail);
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	getDb().delete(user).where(eq(user.id, found.id)).run();
}
