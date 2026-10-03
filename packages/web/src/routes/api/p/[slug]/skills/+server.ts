import { error, json } from '@sveltejs/kit';
import { listProfileSkills, profileSkillsDir, setSkillsEnabled } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

function skills(profile: { slug: string; disabledSkills: string[] }) {
	return listProfileSkills(profileSkillsDir(profile.slug), profile.disabledSkills).skills.map(
		({ name, description, scope, enabled, tokens }) => ({
			name,
			description,
			scope,
			enabled,
			tokens
		})
	);
}

/** The skills nolune has in the profile, as its Skills page lists them: on unless turned off. */
export const GET: RequestHandler = ({ params, locals }) => {
	const { profile } = requireProfile(locals, params.slug);
	return json({ skills: skills(profile) });
};

/** Turns skills on or off in the profile: `{ names: […], enabled }`. Answers with them all. */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const body = (await request.json().catch(() => null)) as {
		names?: unknown;
		enabled?: unknown;
	} | null;
	if (!Array.isArray(body?.names) || typeof body.enabled !== 'boolean') {
		error(400, 'Send the skills’ names and whether they’re on');
	}
	const known = new Set(skills(profile).map((s) => s.name));
	const names = body.names.filter(
		(name): name is string => typeof name === 'string' && known.has(name)
	);
	if (!names.length) error(400, translations(locals.locale).m.skills.gone);
	setSkillsEnabled(profile.id, names, body.enabled);
	const { profile: updated } = requireProfile(locals, params.slug);
	return json({ skills: skills(updated) });
};
