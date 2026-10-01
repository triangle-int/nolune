/* How the nolune plan's limits read in the bars (plan-usage.svelte.ts keeps the numbers). */

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
