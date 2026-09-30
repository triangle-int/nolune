import type { I18n } from './i18n';

/** What formatting needs of the interface's language. */
type Language = Pick<I18n, 'intl' | 'm'>;

export function formatTokens(n: number | null | undefined): string {
	if (n == null) return '?';
	if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1000) return `${Math.round(n / 1000)}K`;
	return String(n);
}

/** "272k", "1.5m" or "272000" → tokens. NaN for anything else, fractions of a token included. */
export function parseTokens(text: string): number {
	const match = /^(\d+(?:\.\d+)?)\s*([km]?)$/i.exec(text.trim());
	if (!match) return NaN;
	const exponent = { k: 3, m: 6 }[match[2].toLowerCase()] ?? 0;
	const tokens = Number(`${match[1]}e${exponent}`);
	return Number.isSafeInteger(tokens) && tokens > 0 ? tokens : NaN;
}

/** Rounds down, so a partial hit never shows as 100%. */
export function formatPercent(rate: number): string {
	return `${Math.floor(rate * 100)}%`;
}

const relatives = new Map<string, Intl.RelativeTimeFormat>();

/** "3 hours ago", "yesterday". */
export function formatAgo(ms: number, { intl, m }: Language, now = Date.now()): string {
	const seconds = Math.round((ms - now) / 1000);
	if (seconds > -60) return m.time.justNow;
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
	let relative = relatives.get(intl);
	if (!relative) {
		relative = new Intl.RelativeTimeFormat(intl, { numeric: 'auto' });
		relatives.set(intl, relative);
	}
	return relative.format(Math.round(seconds / size), unit);
}

export function formatBytes(bytes: number, { intl, m }: Language): string {
	if (bytes < 1024) return `${bytes} ${m.units.bytes}`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} ${m.units.kilobytes}`;
	const mb = bytes / (1024 * 1024);
	const digits = mb < 10 ? 1 : 0;
	const size = mb.toLocaleString(intl, {
		minimumFractionDigits: digits,
		maximumFractionDigits: digits
	});
	return `${size} ${m.units.megabytes}`;
}
