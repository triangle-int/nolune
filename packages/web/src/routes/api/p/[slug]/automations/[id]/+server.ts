import { error, json } from '@sveltejs/kit';
import { deleteTrigger, getTrigger, setTriggerEnabled, updateTrigger } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/** The automation, if it's the profile's. */
function requireTrigger(locals: App.Locals, params: { slug: string; id: string }) {
	const { profile } = requireProfile(locals, params.slug);
	const { m } = translations(locals.locale);
	const t = getTrigger(params.id);
	if (!t || t.profileId !== profile.id) error(404, m.errors.automationNotFound);
	return { t, m };
}

/**
 * Changes an automation as its page does: `{ summary, text }` (its description, and its
 * instructions or command), or `{ enabled }` to pause or resume it. Answers with what happened.
 */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	const { t, m } = requireTrigger(locals, params);
	const body = (await request.json().catch(() => null)) as {
		summary?: unknown;
		text?: unknown;
		enabled?: unknown;
	} | null;
	try {
		if (typeof body?.enabled === 'boolean') {
			setTriggerEnabled(t.id, body.enabled);
			return json({
				message: body.enabled ? m.automations.resumed(t.name) : m.automations.paused(t.name)
			});
		}
		if (typeof body?.text !== 'string') error(400, 'Send the instructions');
		updateTrigger(t.id, {
			summary: typeof body.summary === 'string' ? body.summary : undefined,
			what:
				t.action === 'agent'
					? { action: 'agent', prompt: body.text }
					: { action: 'script', command: body.text }
		});
	} catch (err) {
		if (err instanceof Error && !('status' in err)) error(400, err.message);
		throw err;
	}
	return json({ message: m.automations.saved(t.name) });
};

export const DELETE: RequestHandler = ({ params, locals }) => {
	const { t, m } = requireTrigger(locals, params);
	deleteTrigger(t.id);
	return json({ message: m.automations.deleted(t.name) });
};
