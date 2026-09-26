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
	listTriggers,
	runTriggerNow,
	setTriggerEnabled,
	updateTrigger,
	webhookUrl,
	type Trigger
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

/** Days the "Coming up" calendar covers, today included. */
const AGENDA_DAYS = 7;
/** A trigger that runs more often than this in those days is listed once, not at every time. */
const AGENDA_MAX_RUNS = 28;

function capitalize(text: string): string {
	return text.charAt(0).toUpperCase() + text.slice(1);
}

/** "Every weekday at 07:30", "Once, tomorrow at 17:00". Times are the gateway's, like cron's. */
function schedule(t: Trigger, now: Date): string {
	if (t.kind === 'cron') return (t.cron && describeCron(t.cron)) || 'On a custom schedule';
	if (t.kind === 'once') return t.runAt ? `Once, ${formatDayTime(t.runAt, now)}` : 'Once';
	return 'When another app calls its link';
}

type AgendaItem = { id: string; name: string; icon: string | null; kind: Trigger['kind'] };

/** What runs from now to the end of the last day, grouped by day, and what runs too often to list. */
function agenda(triggers: Trigger[], now: Date) {
	const until = new Date(now.getFullYear(), now.getMonth(), now.getDate() + AGENDA_DAYS);
	const runs: (AgendaItem & { at: Date })[] = [];
	const frequent: (AgendaItem & { schedule: string })[] = [];
	for (const t of triggers) {
		if (!t.enabled || isFinished(t)) continue;
		const item = { id: t.id, name: t.name, icon: t.icon, kind: t.kind };
		if (t.kind === 'cron' && t.cron) {
			const times = cronRunsBetween(t.cron, now, until, AGENDA_MAX_RUNS + 1);
			if (times.length > AGENDA_MAX_RUNS) frequent.push({ ...item, schedule: schedule(t, now) });
			else runs.push(...times.map((at) => ({ ...item, at })));
		} else if (t.kind === 'once' && t.runAt && t.runAt >= now && t.runAt < until) {
			runs.push({ ...item, at: t.runAt });
		}
	}
	runs.sort((a, b) => a.at.getTime() - b.at.getTime());
	const days = new Map<
		string,
		{ key: string; label: string; date: string; items: (AgendaItem & { time: string })[] }
	>();
	for (const { at, ...item } of runs) {
		const key = dayKey(at);
		let day = days.get(key);
		if (!day) {
			day = { key, label: capitalize(formatDay(at, now)), date: formatDate(at), items: [] };
			days.set(key, day);
		}
		day.items.push({ ...item, time: formatClock(at) });
	}
	return { days: [...days.values()], frequent };
}

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const now = new Date();
	const triggers = listTriggers(profile.id);
	return {
		timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
		agenda: agenda(triggers, now),
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
