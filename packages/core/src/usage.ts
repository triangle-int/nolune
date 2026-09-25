// Prompt cache arithmetic. No runtime imports, so the web client can use it too (`@btw/core/usage`).
import type { Usage } from './conversations.ts';

/** Shortfalls smaller than the minimum cacheable prefix aren't worth flagging. */
const MISS_THRESHOLD = 1024;

/** How long an unused cache entry lives: the `ttl: '1h'` in anthropic.ts. */
export const CACHE_TTL_MS = 60 * 60 * 1000;

/** The whole prompt of one call: read from the cache, written to it, or neither. */
export function promptTokens(usage: Usage): number {
	return usage.input + usage.cacheRead + usage.cacheWrite;
}

/** Share of the prompt served from the cache, 0 to 1. */
export function cacheHitRate(usage: Usage): number {
	const total = promptTokens(usage);
	return total === 0 ? 0 : usage.cacheRead / total;
}

/**
 * Tokens the previous call left in the cache that this call had to process again, or 0. History
 * is only appended to, so each call should read everything the call before it read or wrote.
 */
export function cacheMissTokens(previous: Usage, current: Usage): number {
	const missed = previous.cacheRead + previous.cacheWrite - current.cacheRead;
	return missed >= MISS_THRESHOLD ? missed : 0;
}
