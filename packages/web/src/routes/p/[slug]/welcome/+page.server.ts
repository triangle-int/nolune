import { error, fail } from '@sveltejs/kit';
import {
	ApiKeyError,
	CLAUDE_INSTALL_COMMAND,
	EXPORT_PROMPT,
	MAX_EXPORT_CHARS,
	addPreset,
	apiKeyStatuses,
	chatGptPlanStatus,
	checkApiKey,
	claudePlanStatus,
	getDefaultPreset,
	cardOf,
	importMemoryExport,
	isApiKeyProvider,
	isPlan,
	listPresets,
	membersWithNotes,
	normalizeApiKey,
	nolunePlanOffered,
	nolunePlanStatus,
	parseMemoryExport,
	reformatMemoryExport,
	saveApiKey,
	setDefaultPreset,
	setProfileAvatar
} from '@nolune/core';
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
		// The nolune plan, first among the choices once it's offered.
		nolunePlan: needsModel && isAdmin && nolunePlanOffered(),
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

	/**
	 * Whether a plan is signed in and works. The Claude plan signs in on Models & keys or in a
	 * terminal; the ChatGPT plan right in the step (/api/chatgpt/sign-in), and the nolune plan is
	 * linked there too (/api/nolune-plan/sign-in).
	 */
	plan: async ({ locals, request }) => {
		requireAdmin(locals);
		const plan = (await request.formData()).get('plan')?.toString() ?? '';
		if (!isPlan(plan)) error(400, 'Unknown plan');
		const status =
			plan === 'claude-plan'
				? await claudePlanStatus()
				: plan === 'chatgpt-plan'
					? await chatGptPlanStatus({ check: true })
					: await nolunePlanStatus({ check: true });
		if (status.signedIn && !status.problem) return { plan, signedIn: status.signedIn };
		const { m } = translations(locals.locale);
		return fail(400, {
			plan,
			planError:
				status.problem ??
				(plan === 'claude-plan'
					? m.admin.claudeNoAnswer
					: plan === 'chatgpt-plan'
						? m.admin.chatgptNobody
						: m.admin.nolunePlanNotLinked),
			// Who's signed in anyway: the step says what's wrong with their plan rather than asking
			// someone to sign in.
			signedIn: status.signedIn,
			// Only Claude Code is installed.
			installCommand: status.installed ? null : CLAUDE_INSTALL_COMMAND
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
		// Onto their card while it has room, then into their note, unless it isn't known yet which
		// one is theirs.
		const note = membersWithNotes(profile).find((member) => member.id === user.id)?.note;
		const result = importMemoryExport(
			profile.slug,
			user.name,
			facts,
			note ?? undefined,
			cardOf(user.id)
		);
		console.log(
			`[nolune] ${profile.slug} imported ${result.added} memories for ${user.name} (${result.skipped} already known)`
		);
		return { imported: result };
	}
};
