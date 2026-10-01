import { describe, expect, it } from 'vitest';
import { readAccount, updateAccount } from './accounts.ts';
import {
	addExtra,
	admit,
	charge,
	grantPeriod,
	HOUR,
	startPlan,
	type PeriodGrant
} from './limits.ts';
import { addUser, testDb } from './test/db.ts';

const t0 = Date.UTC(2026, 9, 1, 18, 0);
const family: PeriodGrant = {
	source: 'in_1',
	credits: 25_000_000,
	carryOver: 12_500_000,
	limits: { window: 1_800_000, week: 8_750_000 },
	renewsAt: t0 + 30 * 24 * HOUR
};
const chat = { kind: 'chat', use: 'person', continuing: false } as const;

describe('plans in Postgres', () => {
	it('keeps a plan, what it spent and its credits as they were', async () => {
		const db = await testDb();
		const id = await addUser(db);
		expect(await readAccount(db, id)).toBeNull();

		await updateAccount(db, id, () => startPlan(family, t0));
		await updateAccount(db, id, (account) =>
			addExtra(account!, { source: 'cs_1', credits: 10_000_000, expiresAt: t0 + 365 * 24 * HOUR })
		);
		const spent = await updateAccount(db, id, (account) => {
			const admission = admit(account!, chat, t0 + HOUR);
			if (!admission.ok) throw new Error(admission.code);
			return charge(account!, {
				request: chat,
				paidBy: admission.paidBy,
				at: t0 + HOUR,
				cost: 1234
			});
		});

		expect(await readAccount(db, id)).toEqual(spent);
		expect(spent).toMatchObject({
			window: { openedAt: t0 + HOUR, spent: 1234 },
			week: { startedAt: t0, spent: 1234 },
			credits: [
				{ source: 'in_1', kind: 'plan', left: 25_000_000 - 1234, expiresAt: null },
				{ source: 'cs_1', kind: 'extra', left: 10_000_000, expiresAt: t0 + 365 * 24 * HOUR }
			]
		});
	});

	it('writes nothing when a change leaves the plan as it was', async () => {
		const db = await testDb();
		const id = await addUser(db);
		const started = await updateAccount(db, id, () => startPlan(family, t0));
		const again = await updateAccount(db, id, (account) => grantPeriod(account!, family, t0));
		expect(again).toEqual(started);
		expect(await updateAccount(db, 'u1', () => null)).toEqual(started);
	});

	it('adds up what requests that end together spent', async () => {
		const db = await testDb();
		const id = await addUser(db);
		await updateAccount(db, id, () => startPlan(family, t0));
		const spend = (cost: number) =>
			updateAccount(db, id, (account) =>
				charge(account!, { request: chat, paidBy: 'plan', at: t0 + HOUR, cost })
			);
		await Promise.all([spend(100), spend(200), spend(300)]);
		expect((await readAccount(db, id))?.window?.spent).toBe(600);
	});
});
