export function formatTokens(n: number | null | undefined): string {
	if (n == null) return '?';
	if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1000) return `${Math.round(n / 1000)}K`;
	return String(n);
}

/** Rounds down, so a partial hit never shows as 100%. */
export function formatPercent(rate: number): string {
	return `${Math.floor(rate * 100)}%`;
}

const relative = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });

/** "3 hours ago", "yesterday". */
export function formatAgo(ms: number, now = Date.now()): string {
	const seconds = Math.round((ms - now) / 1000);
	if (seconds > -60) return 'just now';
	const steps: [Intl.RelativeTimeFormatUnit, number][] = [
		['minute', 60],
		['hour', 3600],
		['day', 86400],
		['week', 604800],
		['month', 2629800],
		['year', 31557600]
	];
	let [unit, size] = steps[0];
	for (const step of steps) if (-seconds >= step[1]) [unit, size] = step;
	return relative.format(Math.round(seconds / size), unit);
}
