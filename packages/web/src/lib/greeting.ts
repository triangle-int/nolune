import type { Messages } from '$lib/i18n';

export type Greetings = Messages['newChat']['greetings'];

/** The parts of the day the new chat's greeting goes by. */
export type DayPart = 'morning' | 'afternoon' | 'evening' | 'night';

/** Morning from 5:00, afternoon from 12:00, evening from 18:00, night from 23:00. */
export function dayPart(hour: number): DayPart {
	if (hour >= 5 && hour < 12) return 'morning';
	if (hour >= 12 && hour < 18) return 'afternoon';
	if (hour >= 18 && hour < 23) return 'evening';
	return 'night';
}

/**
 * The new chat's greeting: one for this part of the day, or one for any time. `seed`, from 0 up
 * to 1, picks which, so the server's page and the browser's agree. `name` may be empty.
 */
export function greeting(greetings: Greetings, name: string, hour: number, seed: number): string {
	const pool = [...greetings[dayPart(hour)], ...greetings.anytime];
	const index = Math.min(pool.length - 1, Math.max(0, Math.floor(seed * pool.length)));
	return pool[index](name);
}
