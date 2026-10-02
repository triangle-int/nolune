/* How the nolune plan's limits read in the bars (plan-usage.svelte.ts keeps the numbers). */

import type { NolunePlanUsage } from '@nolune/core';

/** A limit's share used, 0 to 100. */
export function usedPercent(spent: number, limit: number): number {
	return limit > 0 ? Math.min(100, Math.round((spent / limit) * 100)) : 0;
}

/** Dollars, from the API's millionths. */
export function dollars(micros: number): string {
	return `$${(micros / 1_000_000).toFixed(2)}`;
}

/** When a limit starts again, as this browser's clock reads it: "18:40", or "Fri 18:40". */
export function whenAgain(ms: number, intl: string, now = new Date()): string {
	const at = new Date(ms);
	const time = at.toLocaleTimeString(intl, { hour: '2-digit', minute: '2-digit' });
	if (at.toDateString() === now.toDateString()) return time;
	return `${at.toLocaleDateString(intl, { weekday: 'short' })} ${time}`;
}

/** One of a plan's limits, as the bars and the note under the composer show it. */
export interface PlanLimit {
	key: 'window' | 'week' | 'month';
	spent: number;
	limit: number;
	resetsAt: number | null;
}

/**
 * The limits a plan has, in the order they're shown: the 5-hour window and the week on a plan that
 * has them, then the month's credits.
 */
export function planLimits(usage: NolunePlanUsage): PlanLimit[] {
	const limits: PlanLimit[] = [];
	if (usage.window) limits.push({ key: 'window', ...usage.window });
	if (usage.week) limits.push({ key: 'week', ...usage.week });
	// An API from before the month was in its answer says none.
	if (usage.month) limits.push({ key: 'month', ...usage.month });
	return limits;
}

/** "resets 18:40" for a window, "renews 1 Nov" for the month; null when it isn't known. */
export function whenItResets(
	limit: PlanLimit,
	intl: string,
	words: { resets: (when: string) => string; renews: (when: string) => string }
): string | null {
	if (!limit.resetsAt) return null;
	if (limit.key !== 'month') return words.resets(whenAgain(limit.resetsAt, intl));
	const day = new Date(limit.resetsAt).toLocaleDateString(intl, { day: 'numeric', month: 'short' });
	return words.renews(day);
}
