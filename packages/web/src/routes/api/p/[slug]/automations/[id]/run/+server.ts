import { error, json } from '@sveltejs/kit';
import { getTrigger, runTriggerNow } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** Runs an automation now, as its page's Run now does. 429 when too many runs wait already. */
export const POST: RequestHandler = ({ params, locals }) => {
	const { profile } = requireProfile(locals, params.slug);
	const { m } = translations(locals.locale);
	const t = getTrigger(params.id);
	if (!t || t.profileId !== profile.id) error(404, m.errors.automationNotFound);
	try {
		runTriggerNow(t);
	} catch (err) {
		error(429, err instanceof Error ? err.message : String(err));
	}
	return json({
		message:
			t.action === 'agent' ? m.automations.started(t.name) : m.automations.startedScript(t.name)
	});
};
