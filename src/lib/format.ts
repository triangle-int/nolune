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
