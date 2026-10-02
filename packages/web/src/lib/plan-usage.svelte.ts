import type { NolunePlanUsage } from '@nolune/core';

/*
 * Where the nolune plan's limits stand, for the bars, asked of the gateway (/api/nolune-plan/usage)
 * when one shows and whenever /api/events says it changed (after each of the plan's requests).
 * Only ever set in the browser: on the server it stays empty, and the bars come once the page has.
 */
class PlanUsage {
	current = $state<{ usage: NolunePlanUsage; at: number } | null>(null);
	#asking = false;
	#again = false;

	async refresh(): Promise<void> {
		if (this.#asking) {
			// Asked while asking: ask once more after, since it may have changed in between.
			this.#again = true;
			return;
		}
		this.#asking = true;
		try {
			const res = await fetch('/api/nolune-plan/usage');
			if (res.ok) this.current = await res.json();
		} catch {
			// The gateway is restarting: the bars keep what they had.
		} finally {
			this.#asking = false;
		}
		if (this.#again) {
			this.#again = false;
			await this.refresh();
		}
	}
}

export const planUsage = new PlanUsage();
