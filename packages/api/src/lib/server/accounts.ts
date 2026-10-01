import { eq } from 'drizzle-orm';
import type { Db } from './db.ts';
import type { Account, Credit } from './limits.ts';
import { credit, plan, user } from './schema.ts';

/*
 * limits.ts's Account, kept in Postgres: the plan's row and its credits. Every change locks the
 * person's row, reads the plan and writes it back in one transaction, so two requests of the same
 * family that end at once each add what they spent rather than one overwriting the other. The
 * lock is on the person rather than the plan, which a first grant has yet to make.
 */

type Tx = Parameters<Parameters<Db['transaction']>[0]>[0];
type PlanRow = typeof plan.$inferSelect;
type CreditRow = typeof credit.$inferSelect;

const ms = (date: Date | null) => (date ? date.getTime() : null);
const date = (ms: number | null) => (ms === null ? null : new Date(ms));

function toAccount(row: PlanRow, credits: CreditRow[]): Account {
	return {
		limits: { window: row.windowLimit, week: row.weekLimit },
		startedAt: row.startedAt.getTime(),
		renewsAt: ms(row.renewsAt),
		window: row.windowOpenedAt
			? { openedAt: row.windowOpenedAt.getTime(), spent: row.windowSpent }
			: null,
		week: row.weekStartedAt
			? { startedAt: row.weekStartedAt.getTime(), spent: row.weekSpent }
			: null,
		credits: credits.map((c): Credit => ({
			source: c.source,
			kind: c.kind,
			left: c.left,
			expiresAt: ms(c.expiresAt)
		})),
		extraPastLimits: row.extraPastLimits
	};
}

async function load(db: Db | Tx, userId: string): Promise<Account | null> {
	const [row] = await db.select().from(plan).where(eq(plan.userId, userId));
	if (!row) return null;
	const credits = await db.select().from(credit).where(eq(credit.userId, userId));
	return toAccount(row, credits);
}

async function save(tx: Tx, userId: string, account: Account): Promise<void> {
	const row = {
		windowLimit: account.limits.window,
		weekLimit: account.limits.week,
		startedAt: new Date(account.startedAt),
		renewsAt: date(account.renewsAt),
		windowOpenedAt: date(account.window?.openedAt ?? null),
		windowSpent: account.window?.spent ?? 0,
		weekStartedAt: date(account.week?.startedAt ?? null),
		weekSpent: account.week?.spent ?? 0,
		extraPastLimits: account.extraPastLimits
	};
	await tx
		.insert(plan)
		.values({ userId, ...row })
		.onConflictDoUpdate({ target: plan.userId, set: row });
	// A handful of rows each: written whole rather than compared.
	await tx.delete(credit).where(eq(credit.userId, userId));
	if (account.credits.length) {
		await tx.insert(credit).values(
			account.credits.map((c) => ({
				userId,
				source: c.source,
				kind: c.kind,
				left: c.left,
				expiresAt: date(c.expiresAt)
			}))
		);
	}
}

/** Someone's plan as it is, or null when they've never had one. */
export function readAccount(db: Db, userId: string): Promise<Account | null> {
	return load(db, userId);
}

/**
 * Changes someone's plan: `change` gets it as it is (null when there's none yet) and returns it as
 * it should be, or null to leave it. Returns what's saved.
 */
export function updateAccount(
	db: Db,
	userId: string,
	change: (account: Account | null) => Account | null
): Promise<Account | null> {
	return db.transaction(async (tx) => {
		await tx.select({ id: user.id }).from(user).where(eq(user.id, userId)).for('update');
		const account = await load(tx, userId);
		const next = change(account);
		if (!next) return account;
		if (next !== account) await save(tx, userId, next);
		return next;
	});
}
