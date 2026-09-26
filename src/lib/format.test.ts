import { describe, expect, it } from 'vitest';
import { formatAgo, formatBytes, formatPercent, formatTokens } from './format';

describe('format', () => {
	it('shortens token counts', () => {
		expect(formatTokens(null)).toBe('?');
		expect(formatTokens(999)).toBe('999');
		expect(formatTokens(12_345)).toBe('12K');
		expect(formatTokens(2_000_000)).toBe('2M');
		expect(formatTokens(1_250_000)).toBe('1.3M');
	});

	it('never rounds a partial hit up to 100%', () => {
		expect(formatPercent(0.999)).toBe('99%');
		expect(formatPercent(1)).toBe('100%');
	});

	it('sizes files', () => {
		expect(formatBytes(512)).toBe('512 B');
		expect(formatBytes(2048)).toBe('2 KB');
		expect(formatBytes(1.5 * 1024 * 1024)).toBe('1.5 MB');
		expect(formatBytes(25 * 1024 * 1024)).toBe('25 MB');
	});

	it('says how long ago', () => {
		const now = Date.UTC(2026, 8, 28, 12);
		expect(formatAgo(now - 30_000, now)).toBe('just now');
		expect(formatAgo(now - 5 * 60_000, now)).toBe('5 minutes ago');
		expect(formatAgo(now - 3 * 3_600_000, now)).toBe('3 hours ago');
		expect(formatAgo(now - 86_400_000, now)).toBe('yesterday');
	});
});
