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
		expect(cacheMissTokens(previous, usage(10, 8000, 500), 'anthropic')).toBe(0);
		expect(cacheMissTokens(previous, usage(10, 7500, 500), 'anthropic')).toBe(0);
		expect(cacheMissTokens(previous, usage(3000, 5000, 500), 'anthropic')).toBe(3000);
	});

	it("expects OpenAI's whole previous prompt back, though its usage reports no writes", () => {
		// Calls of a chat on the ChatGPT plan: two in a row that read nothing.
		expect(cacheMissTokens(usage(169363, 0, 0), usage(170225, 0, 0), 'chatgpt-plan')).toBe(169363);
		// One that read only part of what the call before it sent.
		expect(cacheMissTokens(usage(10584, 55168, 0), usage(56991, 11648, 0), 'openai')).toBe(54104);
		// The cache keeps whole 128-token blocks, so the end of the prompt that didn't fill one isn't
		// a miss.
		expect(cacheMissTokens(usage(3153, 174208, 0), usage(459, 177280, 0), 'chatgpt-plan')).toBe(0);
		// Elsewhere only what the call before read or wrote is expected back.
		expect(cacheMissTokens(usage(169363, 0, 0), usage(170225, 0, 0), 'anthropic')).toBe(0);
		expect(cacheMissTokens(usage(169363, 0, 0), usage(170225, 0, 0), null)).toBe(0);
	});
});
