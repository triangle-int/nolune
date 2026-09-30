import { homedir } from 'node:os';
import { fail } from '@sveltejs/kit';
import {
	listProfileSkills,
	paths,
	profileSkillsDir,
	scanSkills,
	setSkillsEnabled
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

function tildify(path: string): string {
	const home = homedir();
	return path.startsWith(home + '/') ? '~' + path.slice(home.length) : path;
}

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const { skills } = listProfileSkills(profileSkillsDir(profile.slug), profile.disabledSkills);
	return {
		skills: skills.map(({ name, description, scope, enabled, tokens }) => ({
			name,
			description,
			scope,
			enabled,
			tokens
		})),
		folders: {
			profile: tildify(profileSkillsDir(profile.slug)),
			global: tildify(paths.globalSkills)
		}
	};
};

export const actions: Actions = {
	set: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const known = new Set(scanSkills(profileSkillsDir(profile.slug)).skills.map((s) => s.name));
		const names = form
			.getAll('name')
			.map(String)
			.filter((name) => known.has(name));
		if (!names.length) {
			return fail(400, { message: translations(locals.locale).m.skills.gone });
		}
		setSkillsEnabled(profile.id, names, form.get('enabled') === 'on');
	}
};
