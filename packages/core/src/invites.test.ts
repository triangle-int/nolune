import { verifyPassword } from 'better-auth/crypto';
import { eq } from 'drizzle-orm';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDb } from './db/index.ts';
import { account, invite } from './db/schema.ts';
import {
	INVITE_DAYS,
	InviteError,
	acceptInvite,
	createInvite,
	findInvite,
	listInvites,
	revokeInvite
} from './invites.ts';
import { makeUser } from './test/fixtures.ts';
import { EmailError, PasswordError, deleteUser, findUser, setAdmin } from './users.ts';

const PASSWORD = 'correct-Horse-battery-7';
const DAY = 24 * 60 * 60 * 1000;

/** An admin to make invites with. */
function admin(name = 'Max') {
	const made = makeUser(name);
	setAdmin(name, true);
	return made;
}

afterEach(() => {
	vi.useRealTimers();
});

describe('createInvite', () => {
	it('keeps only the hash of the token, and finds the invite by the token', () => {
		const max = admin();
		const { invite: made, token } = createInvite({ name: ' Grandma ', createdBy: max.id });
		expect(token).toMatch(/^[\w-]{32}$/);
		expect(made).toMatchObject({ name: 'Grandma', createdBy: 'Max' });
		expect(made.expiresAt.getTime() - made.createdAt.getTime()).toBe(INVITE_DAYS * DAY);
		const row = getDb().select().from(invite).get()!;
		expect(row.tokenHash).not.toContain(token);
		expect(findInvite(token)).toEqual(made);
		expect(findInvite(`${token}x`)).toBeNull();
	});

	it('lists the ones that work, newest first', () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		const max = admin();
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const old = createInvite({ name: 'Old', createdBy: max.id });
		vi.setSystemTime(Date.UTC(2026, 8, 5));
		createInvite({ name: 'Grandma', createdBy: max.id });
		vi.setSystemTime(Date.UTC(2026, 8, 6));
		createInvite({});
		expect(listInvites().map((i) => [i.name, i.createdBy])).toEqual([
			[null, null],
			['Grandma', 'Max'],
			['Old', 'Max']
		]);
		// A week on, the first one expired, and the next one made clears it away.
		vi.setSystemTime(Date.UTC(2026, 8, 8, 1));
		expect(listInvites().map((i) => i.name)).toEqual([null, 'Grandma']);
		expect(findInvite(old.token)).toBeNull();
		createInvite({ createdBy: max.id });
		expect(getDb().select().from(invite).all()).toHaveLength(3);
	});

	it("stops working when it's taken back, or its admin no longer is one", () => {
		const max = admin();
		const anna = admin('Anna');
		const mine = createInvite({ createdBy: max.id });
		const hers = createInvite({ createdBy: anna.id });
		revokeInvite(mine.invite.id);
		expect(findInvite(mine.token)).toBeNull();
		setAdmin('Anna', false);
		expect(findInvite(hers.token)).toBeNull();
		expect(listInvites()).toEqual([]);
		// Back as an admin, her link works again; removed, it's gone with her.
		setAdmin('Anna', true);
		expect(findInvite(hers.token)).not.toBeNull();
		deleteUser('Anna');
		expect(getDb().select().from(invite).all()).toEqual([]);
	});
});

describe('acceptInvite', () => {
	it('makes an ordinary account and uses the invite up', async () => {
		const max = admin();
		const { token } = createInvite({ name: 'Grandma', createdBy: max.id });
		const { id } = await acceptInvite(token, {
			name: 'Grandma',
			email: 'Grandma@Example.com',
			password: PASSWORD
		});
		expect(findUser('grandma@example.com')).toMatchObject({ id, name: 'Grandma', isAdmin: false });
		const { password } = getDb().select().from(account).where(eq(account.userId, id)).get()!;
		expect(await verifyPassword({ hash: password!, password: PASSWORD })).toBe(true);
		expect(findInvite(token)).toBeNull();
		await expect(
			acceptInvite(token, { name: 'Again', email: 'again@example.com', password: PASSWORD })
		).rejects.toThrow(InviteError);
	});

	it("keeps the invite when the account can't be made", async () => {
		const max = admin();
		const { token } = createInvite({ createdBy: max.id });
		await expect(
			acceptInvite(token, { name: 'Anna', email: 'max@example.com', password: PASSWORD })
		).rejects.toThrow(EmailError);
		await expect(
			acceptInvite(token, { name: 'Anna', email: 'anna@example.com', password: 'short' })
		).rejects.toThrow(PasswordError);
		expect(findInvite(token)).not.toBeNull();
	});

	it('makes one account when a link is sent twice at once', async () => {
		const max = admin();
		const { token } = createInvite({ createdBy: max.id });
		const results = await Promise.allSettled([
			acceptInvite(token, { name: 'Anna', email: 'anna@example.com', password: PASSWORD }),
			acceptInvite(token, { name: 'Ben', email: 'ben@example.com', password: PASSWORD })
		]);
		expect(results.map((r) => r.status).sort()).toEqual(['fulfilled', 'rejected']);
		const failed = results.find((r) => r.status === 'rejected') as PromiseRejectedResult;
		expect(failed.reason).toBeInstanceOf(InviteError);
	});

	it("refuses a link that doesn't work", async () => {
		await expect(
			acceptInvite('nothing', { name: 'Anna', email: 'anna@example.com', password: PASSWORD })
		).rejects.toThrow(InviteError);
		expect(findUser('anna@example.com')).toBeUndefined();
	});
});
