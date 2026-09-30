import { existsSync, readFileSync } from 'node:fs';
import { verifyPassword } from 'better-auth/crypto';
import { eq } from 'drizzle-orm';
import { describe, expect, it } from 'vitest';
import { getDb } from './db/index.ts';
import { account, profileMember } from './db/schema.ts';
import { blobPath, storeBytes } from './media.ts';
import { createProfile, listMembers, noticeProfileChanges, onProfileChanged } from './profiles.ts';
import { makeUser } from './test/fixtures.ts';
import {
	EmailError,
	MAX_PICTURE_BYTES,
	PasswordError,
	PictureError,
	UserNameError,
	clearUserPicture,
	createUser,
	deleteUser,
	findUser,
	listUsers,
	passwordProblem,
	renameUser,
	setAdmin,
	setPassword,
	setUserPicture,
	userPictureFile
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
		await expect(
			createUser({ name: 'max@home', email: 'max@example.com', password: PASSWORD })
		).rejects.toThrow("Name can't contain @");
		await expect(
			createUser({ name: 'Max', email: 'max at example.com', password: PASSWORD })
		).rejects.toThrow('"max at example.com" isn\'t an email address');
		expect(listUsers()).toHaveLength(1);
	});

	it('says why, for the web UI to say it in its language', async () => {
		await createUser({ name: 'Anna', email: 'anna@example.com', password: PASSWORD });
		const reason = (input: { name?: string; email?: string; password?: string }) =>
			createUser({ name: 'Max', email: 'max@example.com', password: PASSWORD, ...input }).then(
				() => null,
				(err: EmailError | PasswordError) => [err.constructor.name, err.reason]
			);
		expect(await reason({ email: 'max' })).toEqual(['EmailError', 'invalid']);
		expect(await reason({ email: 'Anna@example.com' })).toEqual(['EmailError', 'taken']);
		expect(await reason({ password: 'Short1!' })).toEqual(['PasswordError', 'tooShort']);
		expect(await reason({ password: 'alllowercaseletters' })).toEqual([
			'PasswordError',
			'tooSimple'
		]);
		expect(await reason({ password: 'aaaaaaaaaaaaaaaaaaaaaaa' })).toEqual([
			'PasswordError',
			'tooRepetitive'
		]);
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

/** The start of a PNG of that size: all inspectImage reads. */
function png(width: number, height: number, padding = 0): Buffer {
	const head = Buffer.alloc(33 + padding);
	Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex').copy(head);
	head.writeUInt32BE(width, 16);
	head.writeUInt32BE(height, 20);
	return head;
}

describe('renameUser', () => {
	it('renames, trimmed, and tells the profiles they are in', () => {
		const anna = makeUser('Anna');
		const family = createProfile('Family', anna.id);
		const changed: string[] = [];
		const stop = onProfileChanged((id) => changed.push(id));
		noticeProfileChanges();
		try {
			expect(renameUser(anna.id, '  Ann  ')).toBe('Ann');
			expect(findUser('ann')?.id).toBe(anna.id);
			expect(listMembers(family.id).map((m) => m.name)).toEqual(['Ann']);
			expect(changed).toEqual([family.id]);
		} finally {
			stop();
		}
	});

	it('lets someone change the case of their own name, but not take another', () => {
		const anna = makeUser('Anna');
		makeUser('Max');
		expect(renameUser(anna.id, 'ANNA')).toBe('ANNA');
		const problem = (name: string) => {
			try {
				renameUser(anna.id, name);
			} catch (err) {
				return err instanceof UserNameError ? [err.reason, err.message] : err;
			}
		};
		expect(problem('max')).toEqual(['taken', 'A user named "max" already exists']);
		expect(problem('   ')).toEqual(['required', 'Name is required']);
		expect(problem('a'.repeat(65))).toEqual(['tooLong', 'Name must be at most 64 characters']);
		expect(problem('anna@home')).toEqual(['email', "Name can't contain @"]);
		expect(renameUser(anna.id, '🌻'.repeat(64))).toBe('🌻'.repeat(64));
	});
});

describe('profile pictures', () => {
	it("keeps the picture in the media store and serves only pictures that are someone's", () => {
		const anna = makeUser('Anna');
		const family = createProfile('Family', anna.id);
		const changed: string[] = [];
		const stop = onProfileChanged((id) => changed.push(id));
		noticeProfileChanges();
		try {
			const sha256 = setUserPicture(anna.id, png(256, 256));
			expect(readFileSync(blobPath(sha256))).toEqual(png(256, 256));
			expect(listMembers(family.id)).toEqual([{ id: anna.id, name: 'Anna', picture: sha256 }]);
			expect(userPictureFile(sha256)).toEqual({ path: blobPath(sha256), mime: 'image/png' });
			expect(changed).toEqual([family.id]);

			// A file from a chat is in the same store, but no one's picture.
			const other = storeBytes(png(10, 10)).sha256;
			expect(userPictureFile(other)).toBeNull();
			expect(userPictureFile('../config.json')).toBeNull();

			clearUserPicture(anna.id);
			expect(listMembers(family.id)[0].picture).toBeNull();
			expect(userPictureFile(sha256)).toBeNull();
			expect(existsSync(blobPath(sha256))).toBe(true);
			expect(changed).toEqual([family.id, family.id]);
		} finally {
			stop();
		}
	});

	it('refuses what is not a picture, and pictures too large', () => {
		const anna = makeUser('Anna');
		const problem = (data: Buffer) => {
			try {
				setUserPicture(anna.id, data);
			} catch (err) {
				return err instanceof PictureError ? err.reason : err;
			}
		};
		expect(problem(Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>'))).toBe('notPicture');
		expect(problem(png(1025, 16))).toBe('tooLarge');
		expect(problem(png(16, 16, MAX_PICTURE_BYTES))).toBe('tooLarge');
		expect(findUser('Anna')?.picture).toBeNull();
	});
});
