/*
 * The nolune plan's credits and limits (DESIGN.md, The nolune plan): whether a request may go,
 * and what it spent once it's done. Pure functions over one account's state, so the API runs them
 * inside one SQLite transaction per request, and tests run them with a clock of their own.
 *
 * Money is in millionths of a dollar: a request can cost $0.0004, which cents can't hold.
 */

export const HOUR = 60 * 60 * 1000;
/** The short window: it opens with a request and closes 5 hours later. */
export const WINDOW = 5 * HOUR;
export const WEEK = 7 * 24 * HOUR;
/** How far a turn that's going may go past a limit, as a share of the 5-hour limit. */
export const OVERDRAFT_SHARE = 0.1;
/** Where background work stops, as a share of the 5-hour and weekly limits. */
export const BACKGROUND_SHARE = 0.8;

/** Dollars (OpenRouter's `usage.cost`) in millionths, rounded up but not by a float's dust. */
export function micros(dollars: number): number {
	return Math.ceil(Number((dollars * 1_000_000).toFixed(3)));
}

export interface Limits {
	/** What a 5-hour window may spend. */
	window: number;
	week: number;
}

/** Credits from one grant: a period's, what carried over from the last one, or a pack. */
export interface Credit {
	/** What granted them (a Stripe invoice or Checkout Session), which grants them only once. */
	source: string;
	kind: 'plan' | 'extra';
	/** What's left. It goes below zero by what a turn that was finishing went over. */
	left: number;
	/**
	 * When what's left is gone: a pack's, a year after it was bought. A period's credits have none;
	 * the next period replaces them, so a payment Stripe is still retrying doesn't take them away.
	 */
	expiresAt: number | null;
}

export interface Account {
	limits: Limits;
	/** When the plan started: each week starts again on that day, at that hour. */
	startedAt: number;
	/** When the next period's credits come, while the subscription goes on. */
	renewsAt: number | null;
	/** The 5-hour window, from the request that opened it. */
	window: { openedAt: number; spent: number } | null;
	/** What a week spent, and which week (when it started). */
	week: { startedAt: number; spent: number } | null;
	credits: Credit[];
	/** Whether an admin lets people go on with extra credits past a limit. */
	extraPastLimits: boolean;
}

/** Background work: hidden conversations and the exchanges nobody waits on (`X-Nolune-Use`). */
export type Use = 'person' | 'background';

export interface Request {
	kind: 'chat' | 'image' | 'embedding';
	use: Use;
	/** It goes on with a turn that started within the limits (`X-Nolune-Turn: continue`). */
	continuing: boolean;
	/** What its input alone will cost, when that's known before it's sent. */
	inputCost?: number;
}

export type Refusal = 'five_hour_limit' | 'weekly_limit' | 'background_share' | 'credits_spent';

export type Admission =
	{ ok: true; paidBy: Credit['kind'] } | { ok: false; code: Refusal; resetsAt: number | null };

/** When the week `at` is in started: the plan's start, a whole number of weeks on. */
export function weekStart(account: Account, at: number): number {
	return account.startedAt + Math.floor((at - account.startedAt) / WEEK) * WEEK;
}

function openWindow(account: Account, at: number): Account['window'] {
	const window = account.window;
	return window && at < window.openedAt + WINDOW ? window : null;
}

function weekSpent(account: Account, at: number): number {
	return account.week?.startedAt === weekStart(account, at) ? account.week.spent : 0;
}

function live(credit: Credit, at: number): boolean {
	return credit.expiresAt === null || at < credit.expiresAt;
}

function creditsLeft(account: Account, kind: Credit['kind'], at: number): number {
	return account.credits
		.filter((credit) => credit.kind === kind && live(credit, at))
		.reduce((sum, credit) => sum + credit.left, 0);
}

/**
 * Whether a request may go, and what pays for it. The plan's credits, within the limits; past
 * them, a person's request goes on extra credits when an admin allows it. Background work never
 * spends those: they were bought for the people in the family.
 */
export function admit(account: Account, request: Request, now: number): Admission {
	const refusal = planRefusal(account, request, now);
	if (!refusal) return { ok: true, paidBy: 'plan' };
	const extra = creditsLeft(account, 'extra', now);
	if (
		account.extraPastLimits &&
		request.use === 'person' &&
		extra > 0 &&
		(request.inputCost ?? 0) <= extra
	) {
		return { ok: true, paidBy: 'extra' };
	}
	return { ok: false, ...refusal };
}

/** What stops the plan paying for a request: the limit that lasts longest, when more than one does. */
function planRefusal(
	account: Account,
	request: Request,
	now: number
): { code: Refusal; resetsAt: number | null } | null {
	const { limits } = account;
	const over = request.continuing ? Math.floor(limits.window * OVERDRAFT_SHARE) : 0;
	const input = request.inputCost ?? 0;
	// A request fits what's left under a limit when something is, and its input fits in that.
	const fits = (left: number) => left + over > 0 && input <= left + over;

	if (!fits(creditsLeft(account, 'plan', now))) {
		return { code: 'credits_spent', resetsAt: account.renewsAt };
	}
	// Embeddings cost next to nothing, and search by meaning shouldn't stop with a window.
	if (request.kind === 'embedding') return null;

	const weekEnds = weekStart(account, now) + WEEK;
	const weekSpentNow = weekSpent(account, now);
	if (!fits(limits.week - weekSpentNow)) return { code: 'weekly_limit', resetsAt: weekEnds };

	const window = openWindow(account, now);
	const windowEnds = window ? window.openedAt + WINDOW : null;
	const windowSpent = window?.spent ?? 0;
	if (!fits(limits.window - windowSpent)) return { code: 'five_hour_limit', resetsAt: windowEnds };

	// Background work leaves people the last fifth of each limit.
	if (request.use === 'background') {
		const share = (limit: number) => Math.floor(limit * BACKGROUND_SHARE);
		if (!fits(share(limits.week) - weekSpentNow)) {
			return { code: 'background_share', resetsAt: weekEnds };
		}
		if (!fits(share(limits.window) - windowSpent)) {
			return { code: 'background_share', resetsAt: windowEnds };
		}
	}
	return null;
}

export interface Charge {
	request: Request;
	/** What `admit` said pays for it. */
	paidBy: Credit['kind'];
	/** When it was admitted, which is when a window it opens opened. */
	at: number;
	cost: number;
}

/** What a request that's done spent: from its credits, and in the windows the plan's credits count in. */
export function charge(account: Account, { request, paidBy, at, cost }: Charge): Account {
	const credits = spend(account.credits, paidBy, cost, at);
	// Embeddings count only against the month, and extra credits are past the windows anyway.
	if (paidBy === 'extra' || request.kind === 'embedding') return { ...account, credits };

	const window = openWindow(account, at) ?? { openedAt: at, spent: 0 };
	const startedAt = weekStart(account, at);
	const week = account.week?.startedAt === startedAt ? account.week : { startedAt, spent: 0 };
	return {
		...account,
		credits,
		window: { ...window, spent: window.spent + cost },
		week: { ...week, spent: week.spent + cost }
	};
}

/**
 * Takes `cost` from the credits of a kind, the soonest to go first. The last one takes what they
 * don't cover, below zero; with none, a credit that's only that is added.
 */
function spend(credits: Credit[], kind: Credit['kind'], cost: number, at: number): Credit[] {
	const next = credits.map((credit) => ({ ...credit }));
	const usable = next
		.filter((credit) => credit.kind === kind && live(credit, at))
		.sort((a, b) => (a.expiresAt ?? Infinity) - (b.expiresAt ?? Infinity));
	if (!usable.length)
		return [...next, { source: `owed:${kind}`, kind, left: -cost, expiresAt: null }];

	let owed = cost;
	for (const credit of usable) {
		const taken = credit === usable.at(-1) ? owed : Math.min(owed, Math.max(credit.left, 0));
		credit.left -= taken;
		owed -= taken;
		if (!owed) break;
	}
	return next;
}

export interface PeriodGrant {
	/** The paid invoice. */
	source: string;
	credits: number;
	/** The most of what's left that carries over. */
	carryOver: number;
	/** The tier's limits, which come with each period, so a new tier takes effect with it. */
	limits: Limits;
	renewsAt: number | null;
}

/**
 * A paid period: its credits, what's left of the last one (up to the tier's cap) and less what the
 * last one went over. An invoice already granted changes nothing, since Stripe may say it twice.
 */
export function grantPeriod(account: Account, grant: PeriodGrant, now: number): Account {
	if (account.credits.some((credit) => credit.source === grant.source)) return account;
	const left = creditsLeft(account, 'plan', now);
	const carried = Math.min(Math.max(left, 0), grant.carryOver);
	const credits = account.credits.filter((credit) => credit.kind !== 'plan');
	credits.push({
		source: grant.source,
		kind: 'plan',
		left: grant.credits + Math.min(left, 0),
		expiresAt: null
	});
	if (carried) {
		credits.push({
			source: `${grant.source}:carried`,
			kind: 'plan',
			left: carried,
			expiresAt: null
		});
	}
	return { ...account, limits: grant.limits, renewsAt: grant.renewsAt, credits };
}

/** A new plan: its weeks start now, and its first period is granted. */
export function startPlan(grant: PeriodGrant, now: number, previous?: Account): Account {
	const account: Account = {
		limits: grant.limits,
		startedAt: now,
		renewsAt: grant.renewsAt,
		window: null,
		week: null,
		credits: previous?.credits.filter((credit) => credit.kind === 'extra') ?? [],
		extraPastLimits: previous?.extraPastLimits ?? false
	};
	return grantPeriod(account, grant, now);
}

/** Whether someone has a plan going: one that ended keeps its packs, for the next one, and no limits. */
export function hasPlan(account: Account | null): account is Account {
	return !!account && account.limits.week > 0;
}

/** Someone with no plan, yet: no limits to spend within, for packs bought before one (or without). */
export function noPlan(now: number): Account {
	return {
		limits: { window: 0, week: 0 },
		startedAt: now,
		renewsAt: null,
		window: null,
		week: null,
		credits: [],
		extraPastLimits: false
	};
}

/** A subscription that ran out: no more limits to spend within, and its credits are gone. Packs stay. */
export function endPlan(account: Account): Account {
	return {
		...account,
		limits: { window: 0, week: 0 },
		renewsAt: null,
		credits: account.credits.filter((credit) => credit.kind === 'extra')
	};
}

/** A pack of extra credits. A Checkout Session already granted changes nothing. */
export function addExtra(
	account: Account,
	pack: { source: string; credits: number; expiresAt: number }
): Account {
	if (account.credits.some((credit) => credit.source === pack.source)) return account;
	const { source, credits, expiresAt } = pack;
	return {
		...account,
		credits: [...account.credits, { source, kind: 'extra', left: credits, expiresAt }]
	};
}

/** How much of each limit is used and when it starts again: every response says it (`x-nolune-usage`). */
export interface Usage {
	window: { spent: number; limit: number; resetsAt: number | null };
	week: { spent: number; limit: number; resetsAt: number };
	credits: { plan: number; extra: number; renewsAt: number | null };
}

export function usage(account: Account, now: number): Usage {
	const window = openWindow(account, now);
	return {
		window: {
			spent: window?.spent ?? 0,
			limit: account.limits.window,
			resetsAt: window ? window.openedAt + WINDOW : null
		},
		week: {
			spent: weekSpent(account, now),
			limit: account.limits.week,
			resetsAt: weekStart(account, now) + WEEK
		},
		credits: {
			plan: creditsLeft(account, 'plan', now),
			extra: creditsLeft(account, 'extra', now),
			renewsAt: account.renewsAt
		}
	};
}
