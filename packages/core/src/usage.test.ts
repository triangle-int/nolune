import { describe, expect, it } from 'vitest';
import { cacheHitRate, cacheMissTokens, promptTokens } from './usage.ts';

const usage = (input: number, cacheRead: number, cacheWrite: number) => ({
	input,
	cacheRead,
	cacheWrite,
	output: 0
});

describe('prompt cache arithmetic', () => {
	it('counts the whole prompt and the share read from the cache', () => {
		expect(promptTokens(usage(100, 700, 200))).toBe(1000);
		expect(cacheHitRate(usage(100, 700, 200))).toBe(0.7);
		expect(cacheHitRate(usage(0, 0, 0))).toBe(0);
	});

	it('flags only misses big enough to matter', () => {
		const previous = usage(10, 5000, 3000);
		expect(cacheMissTokens(previous, usage(10, 8000, 500))).toBe(0);
		expect(cacheMissTokens(previous, usage(10, 7500, 500))).toBe(0);
		expect(cacheMissTokens(previous, usage(3000, 5000, 500))).toBe(3000);
	});
});
