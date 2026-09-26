import { describe, expect, it } from 'vitest';
import { dayKey, describeCron, formatDay } from './schedule.ts';

describe('describeCron', () => {
	it.each([
		['30 7 * * 1-5', 'Every weekday at 07:30'],
		['*/10 9-17 * * *', 'Every 10 minutes from 09:00 to 17:50'],
		['0 9-17 * * *', 'Every hour from 09:00 to 17:00'],
		['0 */2 * * *', 'Every 2 hours'],
		['15 * * * *', 'Every hour at :15'],
		['@hourly', 'Every hour'],
		['@daily', 'Every day at 00:00'],
		['0 9 * * 6,0', 'On weekends at 09:00'],
		['0 9 * * 1-4', 'Monday to Thursday at 09:00'],
		['0 9,18 * * 1,3,5', 'Every Monday, Wednesday and Friday at 09:00 and 18:00'],
		['0 7 * * sun', 'Every Sunday at 07:00'],
		['0 7 * * 7', 'Every Sunday at 07:00'],
		['0 8 1 * *', 'On the 1st of every month at 08:00'],
		['0 9 25 12 *', 'Every year on 25 December at 09:00'],
		['0 9 * 6-8 *', 'Every day from June to August at 09:00']
	])('%s is "%s"', (expr, words) => {
		expect(describeCron(expr)).toBe(words);
	});

	it.each([
		['syntax it does not read', '0 9 L * *'],
		['both day of month and weekday', '0 9 1 * 1'],
		['uneven steps', '*/7 * * * *'],
		['not cron', 'nonsense']
	])('gives up on %s', (_, expr) => {
		expect(describeCron(expr)).toBeNull();
	});
});

describe('formatDay', () => {
	const now = new Date(2026, 8, 28, 12, 0);

	it('names nearby days', () => {
		expect(formatDay(new Date(2026, 8, 28, 7, 30), now)).toBe('today');
		expect(formatDay(new Date(2026, 8, 29, 23, 59), now)).toBe('tomorrow');
		expect(formatDay(new Date(2026, 8, 27, 0, 0), now)).toBe('yesterday');
		expect(formatDay(new Date(2026, 9, 1), now)).toBe('Thursday');
	});

	it('dates days further away, with the year when it differs', () => {
		expect(formatDay(new Date(2026, 9, 10), now)).toBe('Sat 10 Oct');
		expect(formatDay(new Date(2027, 0, 5), now)).toBe('Tue 5 Jan 2027');
	});
});

describe('dayKey', () => {
	it('is the local calendar day', () => {
		expect(dayKey(new Date(2026, 0, 5, 23, 59))).toBe('2026-01-05');
	});
});
