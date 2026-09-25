import { error, fail } from '@sveltejs/kit';
import {
	deleteTrigger,
	describeWhen,
	formatLocalTime,
	getPreset,
	getTrigger,
	isFinished,
	listRuns,
	listTriggers,
	runTriggerNow,
	setTriggerEnabled,
	updateTrigger,
	webhookUrl
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	return {
		timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
		triggers: listTriggers(profile.id).map((t) => ({
			id: t.id,
			name: t.name,
			when: describeWhen(t),
			state: isFinished(t) ? ('done' as const) : t.enabled ? ('on' as const) : ('paused' as const),
			// One-time triggers already show their time in `when`.
			next: t.kind === 'cron' && t.nextRunAt ? formatLocalTime(t.nextRunAt) : null,
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
				at: formatLocalTime(r.createdAt),
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
