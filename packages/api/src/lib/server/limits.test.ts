import { describe, expect, it } from 'vitest';
import {
	addExtra,
	admit,
	charge,
	endPlan,
	DAY,
	grantPeriod,
	hasPlan,
	HOUR,
	micros,
	startPlan,
	usage,
	WEEK,
	WINDOW,
	type Account,
	type PeriodGrant,
	type Request
} from './limits.ts';

// The launch offer's Family plan: $25 of credits, $12.50 of them carried over, and no windows.
const t0 = Date.UTC(2026, 9, 1, 18, 0);
const family: PeriodGrant = {
	source: 'in_1',
	credits: 25_000_000,
	carryOver: 12_500_000,
	limits: { window: null, week: null },
	renewsAt: t0 + 30 * 24 * HOUR
};
// A tier with windows too: $1.80 a 5-hour window, $8.75 a week.
const windowed: PeriodGrant = { ...family, limits: { window: 1_800_000, week: 8_750_000 } };
const chat: Request = { kind: 'chat', use: 'person', continuing: false };
const background: Request = { ...chat, use: 'background' };
const continuing: Request = { ...chat, continuing: true };
const embedding: Request = { kind: 'embedding', use: 'background', continuing: false };

/** Spends `cost` on a request admitted `at`, as the API does once it's done. */
function spent(account: Account, cost: number, at: number, request = chat): Account {
	const admission = admit(account, request, at);
	if (!admission.ok) throw new Error(`refused: ${admission.code}`);
	return charge(account, { request, paidBy: admission.paidBy, at, cost });
}

describe('the nolune plan’s limits', () => {
	it('lets people spend the month’s credits as they like, with no window unless a tier has one', () => {
		let account = startPlan(family, t0);
		account = spent(account, 10_000_000, t0 + HOUR);
		account = spent(account, 10_000_000, t0 + 2 * HOUR);
		expect(admit(account, chat, t0 + 2 * HOUR)).toEqual({ ok: true, paidBy: 'plan' });
		expect(usage(account, t0 + 2 * HOUR)).toEqual({
			window: null,
			week: null,
			month: { spent: 20_000_000, limit: 25_000_000, resetsAt: family.renewsAt },
			credits: { plan: 5_000_000, extra: 0, renewsAt: family.renewsAt }
		});
		account = spent(account, 5_000_000, t0 + 3 * HOUR);
		expect(admit(account, chat, t0 + 3 * HOUR)).toEqual({
			ok: false,
			code: 'credits_spent',
			resetsAt: family.renewsAt
		});
	});

	it('lets background work spend a tenth of the credits in a day, and keeps the rest for people', () => {
		let account = startPlan(family, t0);
		account = spent(account, 1_500_000, t0 + HOUR, background);
		account = spent(account, 1_000_000, t0 + 2 * HOUR, background);
		expect(admit(account, background, t0 + 3 * HOUR)).toEqual({
			ok: false,
			code: 'background_limit',
			resetsAt: t0 + HOUR + DAY
		});
		// People go on, and what they spend doesn't count against background work's day.
		expect(admit(account, chat, t0 + 3 * HOUR)).toEqual({ ok: true, paidBy: 'plan' });
		account = spent(account, 3_000_000, t0 + 3 * HOUR);
		expect(admit(account, background, t0 + HOUR + DAY)).toEqual({ ok: true, paidBy: 'plan' });
		// A turn that's going may finish, by a tenth of that day's limit.
		expect(admit(account, { ...background, continuing: true }, t0 + 3 * HOUR)).toEqual({
			ok: true,
			paidBy: 'plan'
		});
	});

	it('opens the 5-hour window with a request and refuses past its limit until it closes', () => {
		let account = startPlan(windowed, t0);
		account = spent(account, 1_000_000, t0 + HOUR);
		account = spent(account, 800_000, t0 + 2 * HOUR);
		expect(admit(account, chat, t0 + 3 * HOUR)).toEqual({
			ok: false,
			code: 'five_hour_limit',
			resetsAt: t0 + HOUR + WINDOW
		});
		expect(admit(account, chat, t0 + HOUR + WINDOW)).toEqual({ ok: true, paidBy: 'plan' });
		expect(usage(account, t0 + 3 * HOUR).window).toEqual({
			spent: 1_800_000,
			limit: 1_800_000,
			resetsAt: t0 + HOUR + WINDOW
		});
	});

	it('starts each week again on the day and at the hour the plan started', () => {
		let account = startPlan(windowed, t0);
		for (let day = 0; day < 5; day++) account = spent(account, 1_750_000, t0 + day * 24 * HOUR);
		const later = t0 + 5 * 24 * HOUR;
		expect(admit(account, chat, later)).toEqual({
			ok: false,
			code: 'weekly_limit',
			resetsAt: t0 + WEEK
		});
		expect(admit(account, chat, t0 + WEEK)).toEqual({ ok: true, paidBy: 'plan' });
		expect(usage(account, t0 + 10 * 24 * HOUR).week).toEqual({
			spent: 0,
			limit: 8_750_000,
			resetsAt: t0 + 2 * WEEK
		});
	});

	it('lets a turn that is going finish, by up to a tenth of the 5-hour limit', () => {
		let account = spent(startPlan(windowed, t0), 1_800_000, t0);
		expect(admit(account, chat, t0 + HOUR)).toMatchObject({ ok: false });
		account = spent(account, 150_000, t0 + HOUR, continuing);
		expect(admit(account, continuing, t0 + HOUR)).toEqual({ ok: true, paidBy: 'plan' });
		account = spent(account, 40_000, t0 + HOUR, continuing);
		expect(admit(account, continuing, t0 + HOUR)).toEqual({
			ok: false,
			code: 'five_hour_limit',
			resetsAt: t0 + WINDOW
		});
	});

	it('stops background work at 80% of a limit and keeps the rest for people', () => {
		const account = spent(startPlan(windowed, t0), 1_440_000, t0);
		expect(admit(account, background, t0 + HOUR)).toEqual({
			ok: false,
			code: 'background_share',
			resetsAt: t0 + WINDOW
		});
		expect(admit(account, chat, t0 + HOUR)).toEqual({ ok: true, paidBy: 'plan' });
	});

	it('counts embeddings only against the month', () => {
		let account = spent(startPlan(windowed, t0), 1_800_000, t0);
		expect(admit(account, embedding, t0 + HOUR)).toEqual({ ok: true, paidBy: 'plan' });
		account = spent(account, 2_000, t0 + HOUR, embedding);
		expect(usage(account, t0 + HOUR).window?.spent).toBe(1_800_000);
		expect(usage(account, t0 + HOUR).credits.plan).toBe(25_000_000 - 1_802_000);
	});

	it('refuses a request whose input alone wouldn’t fit what’s left', () => {
		const account = spent(startPlan(windowed, t0), 1_700_000, t0);
		expect(admit(account, { ...chat, inputCost: 200_000 }, t0 + HOUR)).toMatchObject({
			ok: false,
			code: 'five_hour_limit'
		});
		expect(admit(account, { ...chat, inputCost: 50_000 }, t0 + HOUR)).toEqual({
			ok: true,
			paidBy: 'plan'
		});
	});

	it('says the limit that lasts longest when more than one is reached', () => {
		const account: Account = {
			...startPlan(windowed, t0),
			window: { openedAt: t0, spent: 1_800_000 },
			week: { startedAt: t0, spent: 8_750_000 }
		};
		expect(admit(account, chat, t0 + HOUR)).toMatchObject({ code: 'weekly_limit' });
		const spentOut = { ...account, credits: [{ ...account.credits[0], left: 0 }] };
		expect(admit(spentOut, chat, t0 + HOUR)).toEqual({
			ok: false,
			code: 'credits_spent',
			resetsAt: windowed.renewsAt
		});
	});

	it('spends extra credits past a limit, on people’s requests, once an admin allows it', () => {
		const pack = { source: 'cs_1', credits: 10_000_000, expiresAt: t0 + 365 * 24 * HOUR };
		let account = addExtra(spent(startPlan(windowed, t0), 1_800_000, t0), pack);
		expect(admit(account, chat, t0 + HOUR)).toMatchObject({ ok: false, code: 'five_hour_limit' });

		account = { ...account, extraPastLimits: true };
		expect(admit(account, chat, t0 + HOUR)).toEqual({ ok: true, paidBy: 'extra' });
		expect(admit(account, background, t0 + HOUR)).toMatchObject({ ok: false });

		account = spent(account, 300_000, t0 + HOUR);
		expect(usage(account, t0 + HOUR)).toMatchObject({
			window: { spent: 1_800_000 },
			credits: { plan: 25_000_000 - 1_800_000, extra: 9_700_000 }
		});
		expect(usage(account, pack.expiresAt).credits.extra).toBe(0);
	});

	it('starts a period with its credits, what carried over up to the cap, less what went over', () => {
		const next = { ...windowed, source: 'in_2' };
		const plenty = grantPeriod(spent(startPlan(windowed, t0), 1_000_000, t0), next, t0 + WEEK);
		expect(usage(plenty, t0 + WEEK).credits.plan).toBe(25_000_000 + 12_500_000);

		const over: Account = {
			...startPlan(windowed, t0),
			credits: [{ source: 'in_1', kind: 'plan', left: -300_000, expiresAt: null }]
		};
		expect(usage(grantPeriod(over, next, t0 + WEEK), t0 + WEEK).credits.plan).toBe(24_700_000);

		expect(grantPeriod(plenty, next, t0 + WEEK)).toBe(plenty);
	});

	it('keeps packs when the subscription ends, and starts the next plan’s weeks anew', () => {
		const pack = { source: 'cs_1', credits: 10_000_000, expiresAt: t0 + 365 * 24 * HOUR };
		const ended = endPlan(addExtra(startPlan(windowed, t0), pack));
		expect(hasPlan(ended)).toBe(false);
		expect(usage(ended, t0 + WEEK).credits).toEqual({ plan: 0, extra: 10_000_000, renewsAt: null });
		expect(admit(ended, chat, t0 + WEEK)).toMatchObject({ ok: false, code: 'credits_spent' });

		const again = startPlan({ ...windowed, source: 'in_9' }, t0 + 3 * 24 * HOUR, ended);
		expect(again.startedAt).toBe(t0 + 3 * 24 * HOUR);
		expect(usage(again, t0 + WEEK).credits.extra).toBe(10_000_000);
	});

	it('counts dollars in millionths, rounded up', () => {
		expect(micros(0.0012)).toBe(1200);
		expect(micros(0.00000001)).toBe(1);
		expect(micros(1.8)).toBe(1_800_000);
	});
});
