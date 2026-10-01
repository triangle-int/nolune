import { describe, expect, it } from 'vitest';
import { dollars, usedPercent, whenAgain } from './usage-format';

describe("the nolune plan's limits, as the bars read them", () => {
	it('says how much of a limit is used, up to all of it', () => {
		expect(usedPercent(0, 1_800_000)).toBe(0);
		expect(usedPercent(1_530_000, 1_800_000)).toBe(85);
		expect(usedPercent(1_990_000, 1_800_000)).toBe(100);
		expect(usedPercent(5, 0)).toBe(0);
	});

	it('says credits in dollars', () => {
		expect(dollars(24_987_382)).toBe('$24.99');
		expect(dollars(0)).toBe('$0.00');
	});

	it('says when a limit starts again: the time today, the day and time later', () => {
		const now = new Date(2026, 9, 1, 14, 30);
		expect(whenAgain(new Date(2026, 9, 1, 18, 40).getTime(), 'en-GB', now)).toBe('18:40');
		expect(whenAgain(new Date(2026, 9, 2, 9, 5).getTime(), 'en-GB', now)).toBe('Fri 09:05');
	});
});
