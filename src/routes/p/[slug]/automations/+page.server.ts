import { error, fail } from '@sveltejs/kit';
import {
	cronRunsBetween,
	dayKey,
	deleteTrigger,
	describeCron,
	formatClock,
	formatDate,
	formatDay,
	formatDayTime,
	getPreset,
	getTrigger,
	isFinished,
	listRuns,
	listRunsBetween,
	listTriggers,
	runTriggerNow,
	setTriggerEnabled,
	updateTrigger,
	webhookUrl,
	type RunStatus,
	type Trigger
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

/** A trigger that runs more than this many times a day, on average, is listed once, not every time. */
const MAX_RUNS_PER_DAY = 4;
const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

function capitalize(text: string): string {
	return text.charAt(0).toUpperCase() + text.slice(1);
}

function addDays(date: Date, days: number): Date {
	return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

/** "2026-09". */
function monthKey(date: Date): string {
	return dayKey(date).slice(0, 7);
}

/** "Every weekday at 07:30", "Once, tomorrow at 17:00". Times are the gateway's, like cron's. */
function schedule(t: Trigger, now: Date): string {
	if (t.kind === 'cron') return (t.cron && describeCron(t.cron)) || 'On a custom schedule';
	if (t.kind === 'once') return t.runAt ? `Once, ${formatDayTime(t.runAt, now)}` : 'Once';
	return 'When another app calls its link';
}

/** Runs and their history are kept this long, so the calendar doesn't go back further. */
const HISTORY_DAYS = 30;

type CalendarEntry = {
	/** The trigger, if it still exists: the entry links to its card. */
	triggerId: string | null;
	name: string;
	icon: string | null;
	kind: Trigger['kind'] | null;
	at: Date;
	/** How it went, for runs that already happened. */
	status: RunStatus | null;
	conversationId: string | null;
};

/**
 * A month as whole weeks, Monday first. Days before now show what ran (agent runs, and scripts
 * that failed: a script that checked and found nothing isn't news); the rest show what will run.
 * Triggers that run too often to show on every day are listed once instead.
 */
function calendar(profileId: string, triggers: Trigger[], month: string | null, now: Date) {
	const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
	const thisMonth = new Date(now.getFullYear(), now.getMonth(), 1);
	const asked = /^(\d{4})-(\d{2})$/.exec(month ?? '');
	const first = asked ? new Date(Number(asked[1]), Number(asked[2]) - 1, 1) : thisMonth;
	const last = new Date(first.getFullYear(), first.getMonth() + 1, 0);
	const gridStart = addDays(first, -((first.getDay() + 6) % 7));
	// The Monday after the last week.
	const gridEnd = addDays(last, 7 - ((last.getDay() + 6) % 7));
	const byId = new Map(triggers.map((t) => [t.id, t]));
	const entries: CalendarEntry[] = [];

	if (gridStart < now) {
		// A broken script fails on every run; like its notification, show that once a day.
		const failedScripts = new Set<string>();
		for (const run of listRunsBetween(profileId, gridStart, gridEnd < now ? gridEnd : now)) {
			if (run.action === 'script') {
				const key = `${dayKey(run.createdAt)} ${run.triggerId}`;
				if (run.status !== 'failed' || failedScripts.has(key)) continue;
				failedScripts.add(key);
			}
			const t = run.triggerId ? byId.get(run.triggerId) : undefined;
			entries.push({
				triggerId: t?.id ?? null,
				name: run.title,
				icon: t?.icon ?? null,
				kind: t?.kind ?? null,
				at: run.createdAt,
				status: run.status,
				conversationId: run.conversationId
			});
		}
	}

	const from = gridStart > now ? gridStart : now;
	const days = Math.ceil((gridEnd.getTime() - from.getTime()) / 86_400_000);
	const frequent: {
		id: string;
		name: string;
		icon: string | null;
		kind: Trigger['kind'];
		schedule: string;
	}[] = [];
	for (const t of days > 0 ? triggers : []) {
		if (!t.enabled || isFinished(t)) continue;
		const entry = {
			triggerId: t.id,
			name: t.name,
			icon: t.icon,
			kind: t.kind,
			status: null,
			conversationId: null
		};
		if (t.kind === 'cron' && t.cron) {
			const limit = MAX_RUNS_PER_DAY * days;
			const times = cronRunsBetween(t.cron, from, gridEnd, limit + 1);
			if (times.length > limit) {
				frequent.push({
					id: t.id,
					name: t.name,
					icon: t.icon,
					kind: t.kind,
					schedule: schedule(t, now)
				});
			} else {
				entries.push(...times.map((at) => ({ ...entry, at })));
			}
		} else if (t.kind === 'once' && t.runAt && t.runAt >= from && t.runAt < gridEnd) {
			entries.push({ ...entry, at: t.runAt });
		}
	}

	entries.sort((a, b) => a.at.getTime() - b.at.getTime());
	const byDay = new Map<string, CalendarEntry[]>();
	for (const entry of entries) {
		const key = dayKey(entry.at);
		if (!byDay.has(key)) byDay.set(key, []);
		byDay.get(key)?.push(entry);
	}

	const cells = [];
	for (let date = gridStart; date < gridEnd; date = addDays(date, 1)) {
		const key = dayKey(date);
		const dayEntries = byDay.get(key) ?? [];
		// One icon per automation, however often it runs that day.
		const icons = [
			...new Map(
				dayEntries.map(({ triggerId, name, icon, kind }) => [
					triggerId ?? name,
					{ key: triggerId ?? name, icon, kind }
				])
			).values()
		];
		const relative = formatDay(date, now);
		cells.push({
			key,
			day: date.getDate(),
			inMonth: date.getMonth() === first.getMonth(),
			isToday: key === dayKey(today),
			isPast: date < today,
			title: `${date.toLocaleDateString('en-GB', { weekday: 'long' })} ${formatDate(date)}`,
			relative: relative === 'today' || relative === 'tomorrow' ? capitalize(relative) : null,
			icons: icons.slice(0, 3),
			more: Math.max(0, icons.length - 3),
			entries: dayEntries.map(({ at, ...entry }) => ({ ...entry, time: formatClock(at) }))
		});
	}

	const oldest = addDays(today, -HISTORY_DAYS);
	const inMonth = cells.filter((c) => c.inMonth);
	const current = first.getTime() === thisMonth.getTime();
	return {
		title: first.toLocaleDateString('en-GB', { month: 'long', year: 'numeric' }),
		weekdays: WEEKDAYS,
		cells,
		frequent,
		prev:
			first > new Date(oldest.getFullYear(), oldest.getMonth(), 1)
				? monthKey(addDays(first, -1))
				: null,
		next: monthKey(addDays(last, 1)),
		current: current ? null : monthKey(thisMonth),
		/** Today in this month, else its first day with something on it. */
		selected: current ? dayKey(today) : (inMonth.find((c) => c.entries.length) ?? inMonth[0]).key
	};
}

export const load: PageServerLoad = ({ locals, params, url }) => {
	const { profile } = requireProfile(locals, params.slug);
	const now = new Date();
	const triggers = listTriggers(profile.id);
	return {
		timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
		calendar: calendar(profile.id, triggers, url.searchParams.get('month'), now),
		triggers: triggers.map((t) => ({
			id: t.id,
			name: t.name,
			summary: t.summary,
			icon: t.icon,
			kind: t.kind,
			schedule: schedule(t, now),
			cron: t.cron,
			state: isFinished(t) ? ('done' as const) : t.enabled ? ('on' as const) : ('paused' as const),
			// One-time triggers already show their time in `schedule`.
			next: t.kind === 'cron' && t.nextRunAt ? formatDayTime(t.nextRunAt, now) : null,
			action: t.action,
			text: (t.action === 'agent' ? t.prompt : t.command) ?? '',
			model: (t.presetId && getPreset(t.presetId)?.name) || 'the first model preset',
			effort: t.effort,
			webhookUrl: t.webhookToken ? webhookUrl(t.webhookToken) : null,
			runs: listRuns(t.id, 5).map((r) => ({
				id: r.id,
				action: r.action,
				source: r.source,
				status: r.status,
				at: capitalize(formatDayTime(r.createdAt, now)),
				conversationId: r.conversationId,
				output: r.output
			}))
		}))
	};
};

function message(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}

/** The trigger named in the form, if it belongs to this profile. */
async function triggerFrom(profileId: string, request: Request) {
	const form = await request.formData();
	const t = getTrigger(form.get('id')?.toString() ?? '');
	if (!t || t.profileId !== profileId) error(404, 'Automation not found');
	return { t, form };
}

export const actions: Actions = {
	edit: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { t, form } = await triggerFrom(profile.id, request);
		const text = form.get('text')?.toString() ?? '';
		try {
			updateTrigger(t.id, {
				summary: form.get('summary')?.toString(),
				what:
					t.action === 'agent'
						? { action: 'agent', prompt: text }
						: { action: 'script', command: text }
			});
		} catch (err) {
			return fail(400, { message: message(err) });
		}
		return { message: `Saved "${t.name}".` };
	},
	run: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { t } = await triggerFrom(profile.id, request);
		try {
			runTriggerNow(t);
		} catch (err) {
			return fail(429, { message: message(err) });
		}
		return {
			message:
				t.action === 'agent'
					? `Started "${t.name}". Its reply shows up under the bell.`
					: `Started the script of "${t.name}".`
		};
	},
	toggle: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { t } = await triggerFrom(profile.id, request);
		try {
			setTriggerEnabled(t.id, !t.enabled);
		} catch (err) {
			return fail(400, { message: message(err) });
		}
		return { message: `"${t.name}" is ${t.enabled ? 'paused' : 'on again'}.` };
	},
	remove: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { t } = await triggerFrom(profile.id, request);
		deleteTrigger(t.id);
		return { message: `Deleted "${t.name}".` };
	}
};
