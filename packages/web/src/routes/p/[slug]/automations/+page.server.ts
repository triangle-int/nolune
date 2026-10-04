import { error, fail } from '@sveltejs/kit';
import {
	deleteTrigger,
	getTrigger,
	runTriggerNow,
	setTriggerEnabled,
	updateTrigger
} from '@nolune/core';
import { translations, type Messages } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { automationsOverview } from '$lib/server/automations';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params, url }) => {
	const { profile } = requireProfile(locals, params.slug);
	return automationsOverview(
		profile.id,
		url.searchParams.get('month'),
		translations(locals.locale)
	);
};

function message(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}

/** The trigger named in the form, if it belongs to this profile. */
async function triggerFrom(profileId: string, request: Request, m: Messages) {
	const form = await request.formData();
	const t = getTrigger(form.get('id')?.toString() ?? '');
	if (!t || t.profileId !== profileId) error(404, m.errors.automationNotFound);
	return { t, form };
}

export const actions: Actions = {
	edit: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const { t, form } = await triggerFrom(profile.id, request, m);
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
		return { message: m.automations.saved(t.name) };
	},
	run: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const { t } = await triggerFrom(profile.id, request, m);
		try {
			runTriggerNow(t);
		} catch (err) {
			return fail(429, { message: message(err) });
		}
		return {
			message:
				t.action === 'agent' ? m.automations.started(t.name) : m.automations.startedScript(t.name)
		};
	},
	toggle: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const { t } = await triggerFrom(profile.id, request, m);
		try {
			setTriggerEnabled(t.id, !t.enabled);
		} catch (err) {
			return fail(400, { message: message(err) });
		}
		return {
			message: t.enabled ? m.automations.paused(t.name) : m.automations.resumed(t.name)
		};
	},
	remove: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const { t } = await triggerFrom(profile.id, request, m);
		deleteTrigger(t.id);
		return { message: m.automations.deleted(t.name) };
	}
};
