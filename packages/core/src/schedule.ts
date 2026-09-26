// Schedules in plain words, for the web UI. The CLI keeps showing cron: the agent edits it.

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const MONTHS = [
	'January',
	'February',
	'March',
	'April',
	'May',
	'June',
	'July',
	'August',
	'September',
	'October',
	'November',
	'December'
];
const DAY_ALIASES = Object.fromEntries(DAYS.map((d, i) => [d.slice(0, 3).toLowerCase(), i]));
const MONTH_ALIASES = Object.fromEntries(
	MONTHS.map((m, i) => [m.slice(0, 3).toLowerCase(), i + 1])
);
const NICKNAMES: Record<string, string> = {
	'@yearly': '0 0 1 1 *',
	'@annually': '0 0 1 1 *',
	'@monthly': '0 0 1 * *',
	'@weekly': '0 0 * * 0',
	'@daily': '0 0 * * *',
	'@midnight': '0 0 * * *',
	'@hourly': '0 * * * *'
};

/**
 * The values a cron field matches, sorted. Null for syntax this doesn't read (`L`, `W`, `#`),
 * which then shows as a custom schedule.
 */
function expand(
	field: string,
	min: number,
	max: number,
	aliases: Record<string, number> = {}
): number[] | null {
	const value = (text: string) => {
		const n = /^\d+$/.test(text) ? Number(text) : aliases[text.toLowerCase()];
		return n !== undefined && n >= min && n <= max ? n : null;
	};
	const found = new Set<number>();
	for (const part of field.split(',')) {
		const match = /^(\*|\?|[a-z0-9]+(?:-[a-z0-9]+)?)(?:\/(\d+))?$/i.exec(part);
		if (!match) return null;
		const [, range, stepText] = match;
		const step = stepText ? Number(stepText) : 1;
		if (step < 1) return null;
		let from = min;
		let to = max;
		if (range !== '*' && range !== '?') {
			const [a, b] = range.split('-');
			const start = value(a);
			const end = b === undefined ? (stepText ? max : start) : value(b);
			if (start === null || end === null || end < start) return null;
			from = start;
			to = end;
		}
		for (let n = from; n <= to; n += step) found.add(n);
	}
	return [...found].sort((a, b) => a - b);
}

/** "a", "a and b", "a, b and c". */
function list(items: string[]): string {
	return items.length < 2
		? (items[0] ?? '')
		: `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

function ordinal(n: number): string {
	const suffix = n % 100 >= 11 && n % 100 <= 13 ? 'th' : ['th', 'st', 'nd', 'rd'][n % 10] || 'th';
	return `${n}${suffix}`;
}

function clock(hour: number, minute: number): string {
	return `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`;
}

/** The step between evenly spaced values that start at `min` and wrap around evenly, if any. */
function stepOf(values: number[], min: number, size: number): number | null {
	if (values.length < 2 || values[0] !== min) return null;
	const step = values[1] - values[0];
	if (size % step !== 0 || values.length !== size / step) return null;
	return values.every((v, i) => v === min + i * step) ? step : null;
}

function isRange(values: number[]): boolean {
	return values.every((v, i) => v === values[0] + i);
}

/** "Monday to Thursday". */
function span(names: string[]): string {
	return `${names[0]} to ${names[names.length - 1]}`;
}

/** Which days, as a sentence start ("Every weekday") and as a tail ("on weekdays"). */
function describeDays(
	dom: number[],
	months: number[],
	dow: number[]
): { every: string; on: string } | null {
	const allDom = dom.length === 31;
	const allMonths = months.length === 12;
	const allDow = dow.length === 7;
	// Cron matches either the day of the month or the weekday when both are set; too subtle to word.
	if (!allDom && !allDow) return null;
	const names = months.map((m) => MONTHS[m - 1]);
	const monthNames = list(names);
	const inMonths =
		names.length >= 3 && isRange(months) ? `from ${span(names)}` : `in ${monthNames}`;

	if (!allDom) {
		if (dom.length > 4) return null;
		const days = list(dom.map(ordinal));
		if (allMonths) {
			return { every: `On the ${days} of every month`, on: `on the ${days} of every month` };
		}
		if (dom.length === 1 && months.length === 1) {
			return {
				every: `Every year on ${dom[0]} ${monthNames}`,
				on: `every year on ${dom[0]} ${monthNames}`
			};
		}
		return { every: `On the ${days} of ${monthNames}`, on: `on the ${days} of ${monthNames}` };
	}

	// Monday first, Sunday last.
	const order = dow.map((d) => (d + 6) % 7).sort((a, b) => a - b);
	const weekdays = order.map((d) => DAYS[(d + 1) % 7]);
	let days: { every: string; on: string };
	if (allDow) days = { every: 'Every day', on: '' };
	else if (order.join() === '0,1,2,3,4') days = { every: 'Every weekday', on: 'on weekdays' };
	else if (order.join() === '5,6') days = { every: 'On weekends', on: 'on weekends' };
	else if (order.length >= 3 && isRange(order)) {
		days = { every: span(weekdays), on: span(weekdays) };
	} else {
		days = {
			every: `Every ${list(weekdays)}`,
			on: `on ${list(weekdays.map((d) => `${d}s`))}`
		};
	}
	if (allMonths) return days;
	return { every: `${days.every} ${inMonths}`, on: `${days.on || 'every day'} ${inMonths}` };
}

/**
 * A 5-field cron expression in plain words: `30 7 * * 1-5` is "Every weekday at 07:30",
 * `*\/10 9-17 * * *` "Every 10 minutes from 09:00 to 17:50". Null when it can't be put simply.
 */
export function describeCron(expr: string): string | null {
	const fields = (NICKNAMES[expr.trim().toLowerCase()] ?? expr).trim().split(/\s+/);
	if (fields.length !== 5) return null;
	const minutes = expand(fields[0], 0, 59);
	const hours = expand(fields[1], 0, 23);
	const dom = expand(fields[2], 1, 31);
	const months = expand(fields[3], 1, 12, MONTH_ALIASES);
	// 7 is Sunday too.
	const dowRaw = expand(fields[4], 0, 7, DAY_ALIASES);
	if (!minutes || !hours || !dom || !months || !dowRaw) return null;
	const dow = [...new Set(dowRaw.map((d) => d % 7))];
	const days = describeDays(dom, months, dow);
	if (!days) return null;

	const tail = days.on ? ` ${days.on}` : '';
	const allMinutes = minutes.length === 60;
	const minuteStep = allMinutes ? 1 : stepOf(minutes, 0, 60);
	const allHours = hours.length === 24;
	const hourStep = allHours ? 1 : stepOf(hours, 0, 24);
	const first = hours[0];
	const last = hours[hours.length - 1];

	if (minuteStep) {
		const every = minuteStep === 1 ? 'Every minute' : `Every ${minuteStep} minutes`;
		if (allHours) return every + tail;
		if (isRange(hours)) {
			return `${every} from ${clock(first, 0)} to ${clock(last, 60 - minuteStep)}${tail}`;
		}
	}
	if (minutes.length === 1) {
		const [m] = minutes;
		const past = m ? ` at :${String(m).padStart(2, '0')}` : '';
		if (allHours) return `Every hour${past}${tail}`;
		if (hourStep) return `Every ${hourStep} hours${past}${tail}`;
		if (hours.length >= 5 && isRange(hours)) {
			return `Every hour from ${clock(first, m)} to ${clock(last, m)}${tail}`;
		}
	}
	const times = hours.flatMap((h) => minutes.map((m) => clock(h, m)));
	if (times.length <= 4) return `${days.every} at ${list(times)}`;
	return null;
}

function dayNumber(date: Date): number {
	return Math.round(
		(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) - Date.UTC(1970, 0, 1)) /
			86_400_000
	);
}

/** "today", "tomorrow", "yesterday", "Monday" (within the coming week), else "Mon 5 Oct". */
export function formatDay(date: Date, now: Date = new Date()): string {
	const diff = dayNumber(date) - dayNumber(now);
	if (diff === 0) return 'today';
	if (diff === 1) return 'tomorrow';
	if (diff === -1) return 'yesterday';
	if (diff > 1 && diff < 7) return date.toLocaleDateString('en-GB', { weekday: 'long' });
	const year = date.getFullYear() !== now.getFullYear() ? ` ${date.getFullYear()}` : '';
	return `${formatWeekday(date)} ${formatDate(date)}${year}`;
}

/** "Mon". */
export function formatWeekday(date: Date): string {
	return date.toLocaleDateString('en-GB', { weekday: 'short' });
}

/** "28 Sept". */
export function formatDate(date: Date): string {
	return date.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}

/** "07:30" in local time. */
export function formatClock(date: Date): string {
	return clock(date.getHours(), date.getMinutes());
}

/** "tomorrow at 07:30", "Mon 5 Oct at 07:30". */
export function formatDayTime(date: Date, now: Date = new Date()): string {
	return `${formatDay(date, now)} at ${formatClock(date)}`;
}

/** Local calendar day, for grouping: "2026-09-28". */
export function dayKey(date: Date): string {
	return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
