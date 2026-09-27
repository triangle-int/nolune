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

/**
 * Which days a schedule runs on. Weekdays are JavaScript's numbers (0 is Sunday), Monday first;
 * `monthDays` are days of the month, at most four.
 */
export type ScheduleDays =
	| { kind: 'daily' }
	| { kind: 'weekdays' }
	| { kind: 'weekends' }
	| { kind: 'weekdayRange'; from: number; to: number }
	| { kind: 'weekdayList'; days: number[] }
	| { kind: 'monthDays'; days: number[] }
	| { kind: 'date'; day: number; month: number };

/** When in those days: clock times are "07:30". */
export type ScheduleTimes =
	/** Every `step` minutes, all day or between two times. */
	| { kind: 'minutes'; step: number; between: { from: string; to: string } | null }
	/** Every `step` hours, `minute` past the hour. */
	| { kind: 'hours'; step: number; minute: number }
	/** Every hour from one time to another. */
	| { kind: 'hourRange'; from: string; to: string }
	| { kind: 'at'; times: string[] };

/** A cron expression taken apart into what can be said in words, in any language. */
export interface Schedule {
	days: ScheduleDays;
	/** 1 to 12, or null for every month (a `date` names its own). */
	months: number[] | null;
	times: ScheduleTimes;
}

/** Whether months are three or more in a row, said as "from June to August". */
export function isMonthSpan(months: number[]): boolean {
	return months.length >= 3 && isRange(months);
}

/**
 * Reads a 5-field cron expression. Null when it can't be put simply: syntax this doesn't read,
 * both a day of the month and a weekday, uneven steps, too many times.
 */
export function parseCron(expr: string): Schedule | null {
	const fields = (NICKNAMES[expr.trim().toLowerCase()] ?? expr).trim().split(/\s+/);
	if (fields.length !== 5) return null;
	const minutes = expand(fields[0], 0, 59);
	const hours = expand(fields[1], 0, 23);
	const dom = expand(fields[2], 1, 31);
	const monthList = expand(fields[3], 1, 12, MONTH_ALIASES);
	// 7 is Sunday too.
	const dowRaw = expand(fields[4], 0, 7, DAY_ALIASES);
	if (!minutes || !hours || !dom || !monthList || !dowRaw) return null;
	const dow = [...new Set(dowRaw.map((d) => d % 7))];

	const allDom = dom.length === 31;
	const allDow = dow.length === 7;
	// Cron matches either the day of the month or the weekday when both are set; too subtle to word.
	if (!allDom && !allDow) return null;
	let months = monthList.length === 12 ? null : monthList;
	let days: ScheduleDays;
	if (!allDom) {
		if (dom.length > 4) return null;
		if (months?.length === 1 && dom.length === 1) {
			days = { kind: 'date', day: dom[0], month: months[0] };
			months = null;
		} else days = { kind: 'monthDays', days: dom };
	} else {
		// Monday first, Sunday last.
		const order = dow.map((d) => (d + 6) % 7).sort((a, b) => a - b);
		const weekdays = order.map((d) => (d + 1) % 7);
		if (allDow) days = { kind: 'daily' };
		else if (order.join() === '0,1,2,3,4') days = { kind: 'weekdays' };
		else if (order.join() === '5,6') days = { kind: 'weekends' };
		else if (order.length >= 3 && isRange(order)) {
			days = { kind: 'weekdayRange', from: weekdays[0], to: weekdays[weekdays.length - 1] };
		} else days = { kind: 'weekdayList', days: weekdays };
	}

	const schedule = (times: ScheduleTimes): Schedule => ({ days, months, times });
	const allMinutes = minutes.length === 60;
	const minuteStep = allMinutes ? 1 : stepOf(minutes, 0, 60);
	const allHours = hours.length === 24;
	const hourStep = allHours ? 1 : stepOf(hours, 0, 24);
	const first = hours[0];
	const last = hours[hours.length - 1];

	if (minuteStep) {
		if (allHours) return schedule({ kind: 'minutes', step: minuteStep, between: null });
		if (isRange(hours)) {
			const between = { from: clock(first, 0), to: clock(last, 60 - minuteStep) };
			return schedule({ kind: 'minutes', step: minuteStep, between });
		}
	}
	if (minutes.length === 1) {
		const [m] = minutes;
		if (hourStep) return schedule({ kind: 'hours', step: hourStep, minute: m });
		if (hours.length >= 5 && isRange(hours)) {
			return schedule({ kind: 'hourRange', from: clock(first, m), to: clock(last, m) });
		}
	}
	const times = hours.flatMap((h) => minutes.map((m) => clock(h, m)));
	return times.length <= 4 ? schedule({ kind: 'at', times }) : null;
}

/** Which days, as a sentence start ("Every weekday") and as a tail ("on weekdays"). */
function describeDays({ days, months }: Schedule): { every: string; on: string } {
	const monthNames = (months ?? []).map((m) => MONTHS[m - 1]);
	if (days.kind === 'monthDays') {
		const which = list(days.days.map(ordinal));
		const of = months ? list(monthNames) : 'every month';
		return { every: `On the ${which} of ${of}`, on: `on the ${which} of ${of}` };
	}
	if (days.kind === 'date') {
		const date = `${days.day} ${MONTHS[days.month - 1]}`;
		return { every: `Every year on ${date}`, on: `every year on ${date}` };
	}
	let words: { every: string; on: string };
	if (days.kind === 'daily') words = { every: 'Every day', on: '' };
	else if (days.kind === 'weekdays') words = { every: 'Every weekday', on: 'on weekdays' };
	else if (days.kind === 'weekends') words = { every: 'On weekends', on: 'on weekends' };
	else if (days.kind === 'weekdayRange') {
		const range = span([DAYS[days.from], DAYS[days.to]]);
		words = { every: range, on: range };
	} else {
		const names = days.days.map((d) => DAYS[d]);
		words = { every: `Every ${list(names)}`, on: `on ${list(names.map((d) => `${d}s`))}` };
	}
	if (!months) return words;
	const inMonths = isMonthSpan(months) ? `from ${span(monthNames)}` : `in ${list(monthNames)}`;
	return { every: `${words.every} ${inMonths}`, on: `${words.on || 'every day'} ${inMonths}` };
}

/** A schedule in plain English. The web UI words it in the reader's language (src/lib/i18n). */
export function describeSchedule(schedule: Schedule): string {
	const days = describeDays(schedule);
	const tail = days.on ? ` ${days.on}` : '';
	const { times } = schedule;
	switch (times.kind) {
		case 'minutes': {
			const every = times.step === 1 ? 'Every minute' : `Every ${times.step} minutes`;
			if (!times.between) return every + tail;
			return `${every} from ${times.between.from} to ${times.between.to}${tail}`;
		}
		case 'hours': {
			const past = times.minute ? ` at :${String(times.minute).padStart(2, '0')}` : '';
			const every = times.step === 1 ? 'Every hour' : `Every ${times.step} hours`;
			return `${every}${past}${tail}`;
		}
		case 'hourRange':
			return `Every hour from ${times.from} to ${times.to}${tail}`;
		case 'at':
			return `${days.every} at ${list(times.times)}`;
	}
}

/**
 * A 5-field cron expression in plain words: `30 7 * * 1-5` is "Every weekday at 07:30",
 * `*\/10 9-17 * * *` "Every 10 minutes from 09:00 to 17:50". Null when it can't be put simply.
 */
export function describeCron(expr: string): string | null {
	const schedule = parseCron(expr);
	return schedule && describeSchedule(schedule);
}

function dayNumber(date: Date): number {
	return Math.round(
		(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) - Date.UTC(1970, 0, 1)) /
			86_400_000
	);
}

/**
 * "today", "tomorrow", "yesterday", "Monday" (within the coming week), else "Mon 5 Oct", in
 * `locale`'s words (a BCP 47 tag like `ru`; British English by default).
 */
export function formatDay(date: Date, now: Date = new Date(), locale = 'en-GB'): string {
	const diff = dayNumber(date) - dayNumber(now);
	if (Math.abs(diff) <= 1) {
		return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(diff, 'day');
	}
	if (diff > 1 && diff < 7) return date.toLocaleDateString(locale, { weekday: 'long' });
	const year = date.getFullYear() !== now.getFullYear() ? ` ${date.getFullYear()}` : '';
	return `${formatWeekday(date, locale)} ${formatDate(date, locale)}${year}`;
}

/** "Mon". */
export function formatWeekday(date: Date, locale = 'en-GB'): string {
	return date.toLocaleDateString(locale, { weekday: 'short' });
}

/** "28 Sept". */
export function formatDate(date: Date, locale = 'en-GB'): string {
	return date.toLocaleDateString(locale, { day: 'numeric', month: 'short' });
}

/** "07:30" in local time. */
export function formatClock(date: Date): string {
	return clock(date.getHours(), date.getMinutes());
}

/** Local calendar day, for grouping: "2026-09-28". */
export function dayKey(date: Date): string {
	return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
