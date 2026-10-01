// Prompt cache arithmetic. No runtime imports, so the web client can use it too (`@nolune/core/usage`).
import type { Usage } from './conversations.ts';
import type { Provider } from './models.ts';

/** Shortfalls smaller than the minimum cacheable prefix aren't worth flagging. */
const MISS_THRESHOLD = 1024;

/**
 * Whether the provider caches every call's whole prompt by itself: OpenAI, with a key or on the
 * ChatGPT plan. Its usage doesn't always say what it wrote (the plan's never does).
 */
export function cachesWholePrompt(provider: Provider | null): boolean {
	return provider === 'openai' || provider === 'chatgpt-plan';
}

/** How long an unused cache entry lives: a conversation's `cacheTtl` (5 minutes for subagents). */
export function cacheTtlMs(ttl: '5m' | '1h'): number {
	return ttl === '5m' ? 5 * 60 * 1000 : 60 * 60 * 1000;
}

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
 * is only appended to, so each call should read everything the call before it read or wrote; on
 * OpenAI (`provider`, the previous call's), its whole prompt, whatever its usage says it wrote.
 */
export function cacheMissTokens(
	previous: Usage,
	current: Usage,
	provider: Provider | null
): number {
	const left = cachesWholePrompt(provider)
		? promptTokens(previous)
		: previous.cacheRead + previous.cacheWrite;
	const missed = left - current.cacheRead;
	return missed >= MISS_THRESHOLD ? missed : 0;
}
