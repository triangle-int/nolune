/*
 * Limits for what the account pages do on their own: better-auth limits requests that come
 * through its handler, not the calls form actions make. Sign-in codes cost an email each, and
 * anyone can ask for one to any address, so they're limited by address and by network. Kept in
 * memory: one process, and a restart forgiving everyone is fine.
 */

export class RateLimit {
	private readonly hits = new Map<string, number[]>();

	constructor(
		private readonly max: number,
		private readonly windowMs: number
	) {}

	/** Counts a try for `key`, and whether it's within the limit. */
	allow(key: string, now = Date.now()): boolean {
		const recent = (this.hits.get(key) ?? []).filter((at) => now - at < this.windowMs);
		const allowed = recent.length < this.max;
		if (allowed) recent.push(now);
		this.hits.set(key, recent);
		if (this.hits.size > 10_000) this.forget(now);
		return allowed;
	}

	private forget(now: number) {
		for (const [key, hits] of this.hits) {
			if (hits.every((at) => now - at >= this.windowMs)) this.hits.delete(key);
		}
	}
}

const TEN_MINUTES = 10 * 60 * 1000;

/** Sign-in codes: 3 to an address and 10 from a network every 10 minutes. */
export const codesByEmail = new RateLimit(3, TEN_MINUTES);
export const codesByNetwork = new RateLimit(10, TEN_MINUTES);
/** Tries at a code, besides better-auth's 5 for each code. */
export const signInsByNetwork = new RateLimit(20, TEN_MINUTES);
