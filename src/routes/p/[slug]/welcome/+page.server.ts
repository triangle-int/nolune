import { error, fail } from '@sveltejs/kit';
import {
	ApiKeyError,
	CLAUDE_INSTALL_COMMAND,
	CODEX_INSTALL_COMMAND,
	EXPORT_PROMPT,
	MAX_EXPORT_CHARS,
	addPreset,
	apiKeyStatuses,
	chatGptPlanStatus,
	checkApiKey,
	claudePlanStatus,
	getDefaultPreset,
	importMemoryExport,
	isApiKeyProvider,
	isPlan,
	listPresets,
	normalizeApiKey,
	parseMemoryExport,
	reformatMemoryExport,
	saveApiKey,
	setDefaultPreset,
	setProfileAvatar
} from '@btw/core';
import { translations } from '$lib/i18n';
import { requireAdmin, requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

/*
 * The first minute of a new profile: an intro, a model when there is none yet, the assistant's
 * avatar, and memories brought over from another assistant. Creating a profile lands here. The
 * steps live in the page; each one saves as it's answered, so leaving halfway loses nothing.
 */

export const load: PageServerLoad = ({ locals, params }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const needsModel = listPresets().length === 0;
	const isAdmin = user.isAdmin === true;
	return {
		// Not `profile`: the root layout tints pages that have one, and this page tints itself
		// once the avatar is picked.
		welcome: { slug: profile.slug, name: profile.name, avatar: profile.avatar },
		person: user.name,
		needsModel,
		isAdmin,
		keys:
			needsModel && isAdmin
				? apiKeyStatuses().map((k) => ({
						provider: k.provider,
						label: k.label,
						consoleUrl: k.consoleUrl,
						set: k.source !== null
					}))
				: [],
		exportPrompt: EXPORT_PROMPT
	};
};

export const actions: Actions = {
	/** Checks an API key with its provider and saves it, as Models & keys does. */
	key: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		const provider = form.get('provider')?.toString() ?? '';
		if (!isApiKeyProvider(provider)) error(400, 'Unknown provider');
		try {
			const key = normalizeApiKey(form.get('key')?.toString() ?? '');
			const warning = await checkApiKey(provider, key);
			saveApiKey(provider, key);
			return { provider, keyWarning: warning };
		} catch (err) {
			if (!(err instanceof ApiKeyError)) throw err;
			return fail(400, { provider, keyError: err.message });
		}
	},

	/** Whether a plan's agent is installed and signed in. Signing in happens on Models & keys. */
	plan: async ({ locals, request }) => {
		requireAdmin(locals);
		const plan = (await request.formData()).get('plan')?.toString() ?? '';
		if (!isPlan(plan)) error(400, 'Unknown plan');
		const status = plan === 'claude-plan' ? await claudePlanStatus() : await chatGptPlanStatus();
		if (status.signedIn && !status.problem) return { plan, signedIn: status.signedIn };
		return fail(400, {
			plan,
			planError: status.problem ?? translations(locals.locale).m.admin.claudeNoAnswer,
			installCommand: status.installed
				? null
				: plan === 'claude-plan'
					? CLAUDE_INSTALL_COMMAND
					: CODEX_INSTALL_COMMAND
		});
	},

	/** The first model preset, which new chats start with. */
	model: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		try {
			const preset = await addPreset({
				provider: form.get('provider')?.toString() ?? '',
				model: form.get('model')?.toString() ?? ''
			});
			setDefaultPreset(preset.id);
			return { preset: preset.name };
		} catch (err) {
			return fail(400, { modelError: err instanceof Error ? err.message : String(err) });
		}
	},

	avatar: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const avatar = (await request.formData()).get('avatar')?.toString() ?? '';
		try {
			setProfileAvatar(profile.id, avatar);
		} catch (err) {
			return fail(400, { avatarError: err instanceof Error ? err.message : String(err) });
		}
		return { avatar };
	},

	/**
	 * Saves what another assistant remembered about this person. An export that doesn't follow
	 * the format goes through the default model first, which only rewrites it.
	 */
	remember: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const text = (await request.formData()).get('text')?.toString() ?? '';
		if (!text.trim()) return fail(400, { rememberError: m.welcome.memory.empty });
		if (text.length > MAX_EXPORT_CHARS)
			return fail(400, { rememberError: m.welcome.memory.tooLong });
		let facts = parseMemoryExport(text);
		if (!facts.length) {
			const preset = getDefaultPreset();
			if (!preset) return fail(400, { rememberError: m.welcome.memory.notAnExport });
			try {
				facts = await reformatMemoryExport(text, preset);
			} catch (err) {
				return fail(502, {
					rememberError: m.welcome.memory.couldNotRead(err instanceof Error ? err.message : '')
				});
			}
			if (!facts.length) return fail(400, { rememberError: m.welcome.memory.nothingFound });
		}
		const result = importMemoryExport(profile.slug, user.name, facts);
		console.log(
			`[btw] ${profile.slug} imported ${result.added} memories for ${user.name} (${result.skipped} already known)`
		);
		return { imported: result };
	}
};
