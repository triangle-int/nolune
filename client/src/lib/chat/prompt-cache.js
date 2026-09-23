/**
 * Views of a chat's prompt-cache readout (`PromptCacheStats` from the
 * server): how much of each request's input the provider read back from its
 * cache, and whether the system prompt stayed the same from turn to turn.
 * The chat bar and the context panel read the same sentences from here.
 */

/**
 * @typedef {{ input_tokens: number, cache_read_tokens: number, cache_write_tokens: number, output_tokens: number }} CacheReading
 * @typedef {{ turns: number, changes: number, last_changed_sections: string[] }} SystemPromptStability
 * @typedef {{ last: CacheReading | null, total: CacheReading, requests: number, hits: number, recent: CacheReading[], system_prompt: SystemPromptStability }} PromptCacheStats
 */

/** @param {number} n */
export function formatTokens(n) {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
	return String(n);
}

/**
 * Share of a reading's input read from the cache, in whole percent.
 * @param {CacheReading | null | undefined} reading
 */
export function hitPercent(reading) {
	if (!reading || reading.input_tokens <= 0) return 0;
	return Math.floor((reading.cache_read_tokens * 100) / reading.input_tokens);
}

/**
 * Input neither read from nor written to the cache.
 * @param {CacheReading} reading
 */
export function uncachedTokens(reading) {
	return Math.max(0, reading.input_tokens - reading.cache_read_tokens - reading.cache_write_tokens);
}

/**
 * The chat bar's label for the latest request, or null before there is one.
 * @param {PromptCacheStats | null | undefined} stats
 */
export function cacheChipLabel(stats) {
	const last = stats?.last;
	if (!last || last.input_tokens <= 0) return null;
	return last.cache_read_tokens > 0 ? `cache ${hitPercent(last)}%` : "cache miss";
}

/**
 * One sentence about the latest request.
 * @param {PromptCacheStats | null | undefined} stats
 */
export function lastRequestSentence(stats) {
	const last = stats?.last;
	if (!last) return "No request in this chat since the server started.";
	const input = formatTokens(last.input_tokens);
	if (last.cache_read_tokens <= 0) {
		const written = last.cache_write_tokens > 0
			? `; ${formatTokens(last.cache_write_tokens)} were written for the next request`
			: "";
		return `Last request missed the cache: none of its ${input} input tokens came from it${written}.`;
	}
	return `Last request: ${hitPercent(last)}% from cache, ${formatTokens(last.cache_read_tokens)} of ${input} input tokens.`;
}

/**
 * A reading split into what was read, written and left uncached, each with
 * its share of the input for a stacked bar.
 * @param {CacheReading} reading
 */
export function cacheSegments(reading) {
	const total = reading.input_tokens;
	const share = (/** @type {number} */ tokens) => (total > 0 ? (tokens / total) * 100 : 0);
	return [
		{ kind: "read", label: "read from cache", tokens: reading.cache_read_tokens },
		{ kind: "write", label: "written to cache", tokens: reading.cache_write_tokens },
		{ kind: "uncached", label: "uncached", tokens: uncachedTokens(reading) },
	].map((segment) => ({ ...segment, percent: share(segment.tokens) }));
}

/**
 * The chat's totals since the server started.
 * @param {PromptCacheStats | null | undefined} stats
 */
export function sessionSentence(stats) {
	if (!stats || stats.requests === 0) return "No request in this chat since the server started.";
	const requests = stats.requests === 1 ? "1 request" : `${stats.requests} requests`;
	return `Since the server started: ${stats.hits} of ${requests} hit the cache, ${hitPercent(stats.total)}% of all ${formatTokens(stats.total.input_tokens)} input tokens came from it.`;
}

/**
 * One request of the recent strip, for its title.
 * @param {CacheReading} reading
 * @param {number} index 0-based, oldest first
 */
export function recentRequestLabel(reading, index) {
	const verdict = reading.cache_read_tokens > 0 ? `${hitPercent(reading)}% from cache` : "cache miss";
	return `Request ${index + 1}: ${verdict}, ${formatTokens(reading.input_tokens)} input tokens`;
}

/**
 * Whether the system prompt changed between turns, and how to say so.
 * @param {PromptCacheStats | null | undefined} stats
 */
export function systemPromptStatus(stats) {
	const prompt = stats?.system_prompt;
	if (!prompt || prompt.turns === 0) {
		return { changed: false, text: "No turn in this chat since the server started." };
	}
	if (prompt.changes === 0) {
		return {
			changed: false,
			text: prompt.turns === 1
				? "System prompt sent once; the next turn is compared with it."
				: `System prompt unchanged across ${prompt.turns} turns.`,
		};
	}
	const times = prompt.changes === 1 ? "once" : `${prompt.changes} times`;
	const where = prompt.last_changed_sections.length > 0
		? `, last in ${prompt.last_changed_sections.join(", ")}`
		: "";
	return {
		changed: true,
		text: `System prompt changed ${times} in ${prompt.turns} turns${where}. Each change makes the next request miss the cache.`,
	};
}
