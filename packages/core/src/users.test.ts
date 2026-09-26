import { verifyPassword } from 'better-auth/crypto';
import { eq } from 'drizzle-orm';
import { describe, expect, it } from 'vitest';
import { getDb } from './db/index.ts';
import { account, profileMember } from './db/schema.ts';
import { createProfile, listMembers } from './profiles.ts';
import {
	createUser,
	deleteUser,
	findUser,
	listUsers,
	passwordProblem,
	setAdmin,
	setPassword
} from './users.ts';

const PASSWORD = 'correct-Horse-battery-7';

function credential(userId: string) {
	return getDb().select().from(account).where(eq(account.userId, userId)).get();
}

describe('passwordProblem', () => {
	it.each([
		['Short1!', 'must be at least 14 characters'],
		[
			'alllowercaseletters',
			'needs 3 of: lowercase, uppercase, digits, symbols (or use 20+ characters)'
		],
		['aaaaaaaaaaaaaaaaaaaaaaa', 'has too few distinct characters']
	])('rejects %s', (password, problem) => {
		expect(passwordProblem(password)).toBe(problem);
	});

	it('accepts mixed passwords and long passphrases', () => {
		expect(passwordProblem(PASSWORD)).toBeNull();
		expect(passwordProblem('purple elephants dance quietly')).toBeNull();
	});
});

describe('createUser', () => {
	it('creates a user with a password account better-auth can sign in with', async () => {
		const { id } = await createUser({
			name: ' Anna ',
			email: ' Anna@Example.com ',
			password: PASSWORD
		});
		expect(findUser('anna@example.com')).toMatchObject({ id, name: 'Anna', isAdmin: false });
		const row = credential(id);
		expect(row).toMatchObject({ providerId: 'credential', accountId: id });
		expect(await verifyPassword({ hash: row!.password!, password: PASSWORD })).toBe(true);
	});

	it('refuses a taken name or email, and weak passwords', async () => {
		await createUser({ name: 'Anna', email: 'anna@example.com', password: PASSWORD });
		await expect(
			createUser({ name: 'ANNA', email: 'other@example.com', password: PASSWORD })
		).rejects.toThrow('A user named "ANNA" already exists');
		await expect(
			createUser({ name: 'Max', email: 'ANNA@example.com', password: PASSWORD })
		).rejects.toThrow('A user with email anna@example.com already exists');
		await expect(
			createUser({ name: 'Max', email: 'max@example.com', password: 'short' })
		).rejects.toThrow('Password must be at least 14 characters');
		expect(listUsers()).toHaveLength(1);
	});
});

describe('changing users', () => {
	it('finds users by name or email, ignoring case', async () => {
		const { id } = await createUser({
			name: 'Anna',
			email: 'anna@example.com',
			password: PASSWORD
		});
		expect(findUser('aNNa')?.id).toBe(id);
		expect(findUser('ANNA@EXAMPLE.COM')?.id).toBe(id);
		expect(findUser('Max')).toBeUndefined();
	});

	it('sets a new password', async () => {
		const { id } = await createUser({
			name: 'Anna',
			email: 'anna@example.com',
			password: PASSWORD
		});
		await setPassword('Anna', 'another-Secret-phrase-9');
		const { password } = credential(id)!;
		expect(await verifyPassword({ hash: password!, password: 'another-Secret-phrase-9' })).toBe(
			true
		);
		expect(await verifyPassword({ hash: password!, password: PASSWORD })).toBe(false);
		await expect(setPassword('Max', PASSWORD)).rejects.toThrow('No user "Max"');
	});

	it('makes admins and lists users by name', async () => {
		await createUser({ name: 'Max', email: 'max@example.com', password: PASSWORD });
		await createUser({ name: 'Anna', email: 'anna@example.com', password: PASSWORD });
		setAdmin('max@example.com', true);
		expect(listUsers().map((u) => [u.name, u.isAdmin])).toEqual([
			['Anna', false],
			['Max', true]
		]);
	});

	it('deletes a user with their accounts and memberships', async () => {
		const { id } = await createUser({
			name: 'Anna',
			email: 'anna@example.com',
			password: PASSWORD
		});
		const family = createProfile('Family', id);
		deleteUser('Anna');
		expect(findUser('Anna')).toBeUndefined();
		expect(credential(id)).toBeUndefined();
		expect(listMembers(family.id)).toEqual([]);
		expect(getDb().select().from(profileMember).all()).toEqual([]);
		expect(() => deleteUser('Anna')).toThrow('No user "Anna"');
	});
});
