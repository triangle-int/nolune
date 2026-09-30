import { describe, expect, it } from 'vitest';
import { formatAgo, formatBytes, formatPercent, formatTokens, parseTokens } from './format';
import { translations } from './i18n';

const en = translations('en');
const ru = translations('ru');

describe('format', () => {
	it('shortens token counts', () => {
		expect(formatTokens(null)).toBe('?');
		expect(formatTokens(999)).toBe('999');
		expect(formatTokens(12_345)).toBe('12K');
		expect(formatTokens(2_000_000)).toBe('2M');
		expect(formatTokens(1_250_000)).toBe('1.3M');
	});

	it('reads token counts, shortened or not', () => {
		expect(parseTokens('272000')).toBe(272_000);
		expect(parseTokens(' 272k ')).toBe(272_000);
		expect(parseTokens('1.5M')).toBe(1_500_000);
		expect(parseTokens('1 m')).toBe(1_000_000);
		expect(parseTokens('0.1')).toBeNaN();
		expect(parseTokens('0')).toBeNaN();
		expect(parseTokens('')).toBeNaN();
		expect(parseTokens('200kb')).toBeNaN();
		expect(parseTokens('-5k')).toBeNaN();
	});

	it('never rounds a partial hit up to 100%', () => {
		expect(formatPercent(0.999)).toBe('99%');
		expect(formatPercent(1)).toBe('100%');
	});

	it('sizes files', () => {
		expect(formatBytes(512, en)).toBe('512 B');
		expect(formatBytes(2048, en)).toBe('2 KB');
		expect(formatBytes(1.5 * 1024 * 1024, en)).toBe('1.5 MB');
		expect(formatBytes(25 * 1024 * 1024, en)).toBe('25 MB');
	});

	it('sizes files in the interface language', () => {
		expect(formatBytes(1.5 * 1024 * 1024, ru)).toBe('1,5 МБ');
	});

	it('says how long ago', () => {
		const now = Date.UTC(2026, 8, 28, 12);
		expect(formatAgo(now - 30_000, en, now)).toBe('just now');
		expect(formatAgo(now - 5 * 60_000, en, now)).toBe('5 minutes ago');
		expect(formatAgo(now - 3 * 3_600_000, en, now)).toBe('3 hours ago');
		expect(formatAgo(now - 86_400_000, en, now)).toBe('yesterday');
	});

	it('says how long ago in the interface language', () => {
		const now = Date.UTC(2026, 8, 28, 12);
		expect(formatAgo(now - 30_000, ru, now)).toBe('только что');
		expect(formatAgo(now - 3 * 3_600_000, ru, now)).toBe('3 часа назад');
		expect(formatAgo(now - 86_400_000, ru, now)).toBe('вчера');
	});
});
