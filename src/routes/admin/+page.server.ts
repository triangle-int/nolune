import { error, fail } from '@sveltejs/kit';
import {
	CLAUDE_INSTALL_COMMAND,
	CODEX_INSTALL_COMMAND,
	ApiKeyError,
	PlanError,
	PROVIDERS,
	PROVIDER_LABELS,
	addPreset,
	apiKeyStatuses,
	cancelChatGptSignIn,
	checkApiKey,
	claudePlanStatus,
	chatGptPlanStatus,
	chatGptSignInState,
	effectiveContextWindow,
	findClaudeCode,
	findCodex,
	getDefaultPreset,
	isApiKeyProvider,
	listPresets,
	normalizeApiKey,
	removeApiKey,
	removePreset,
	saveApiKey,
	setDefaultPreset,
	signOutChatGpt,
	startChatGptSignIn,
	type Plan,
	type PlanStatus
} from '@btw/core';
import { parseTokens } from '$lib/format';
import { translations } from '$lib/i18n';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, depends }) => {
	requireAdmin(locals);
	// The page asks again while a ChatGPT sign-in waits for its code.
	depends('btw:chatgpt-plan');
	const defaultId = getDefaultPreset()?.id;
	const codex = findCodex();
	const signIn = chatGptSignInState();
	return {
		// Where each key comes from and its last four characters; never the keys themselves.
		keys: apiKeyStatuses(),
		providers: PROVIDERS.map((id) => ({ id, label: PROVIDER_LABELS[id] })),
		// Where Claude Code is; whether it's signed in takes starting it, so that's a button.
		claude: { ...findClaudeCode(), installCommand: CLAUDE_INSTALL_COMMAND },
		// Where Codex is, who it's signed in as (asking takes starting it, which waits while a
		// sign-in's code does), and a sign-in's code. Never the sign-in: Codex keeps it.
		chatgpt: {
			...codex,
			...signIn,
			installCommand: CODEX_INSTALL_COMMAND,
			status: codex.installed && !signIn.pending ? await chatGptPlanStatus() : null
		},
		presets: listPresets().map((p) => ({
			id: p.id,
			name: p.name,
			provider: p.provider,
			model: p.model,
			contextWindow: effectiveContextWindow(p),
			overridden: p.contextWindow != null,
			isDefault: p.id === defaultId
		}))
	};
};

/** A plan's status as a form result, for the row of the plan it's about. */
function planResult(plan: Plan, status: PlanStatus, locale: App.Locals['locale']) {
	if (status.problem || !status.signedIn) {
		const { m } = translations(locale);
		return fail(400, { plan, planError: status.problem ?? m.admin.claudeNoAnswer });
	}
	const { signedIn } = status;
	return { plan, planMessage: `${signedIn[0].toUpperCase()}${signedIn.slice(1)}.` };
}

/** The provider a key form is about, or a 400. */
async function keyForm(request: Request) {
	const form = await request.formData();
	const provider = form.get('provider')?.toString() ?? '';
	if (!isApiKeyProvider(provider)) error(400, 'Unknown provider');
	return { provider, key: form.get('key')?.toString() ?? '' };
}

export const actions: Actions = {
	saveKey: async ({ locals, request }) => {
		requireAdmin(locals);
		const { provider, key: pasted } = await keyForm(request);
		try {
			const key = normalizeApiKey(pasted);
			const warning = await checkApiKey(provider, key);
			saveApiKey(provider, key);
			const { m } = translations(locals.locale);
			return {
				provider,
				keyMessage: warning ? m.admin.savedWarning(warning) : m.admin.savedWorks
			};
		} catch (err) {
			if (!(err instanceof ApiKeyError)) throw err;
			return fail(400, { provider, keyError: err.message });
		}
	},
	checkPlan: async ({ locals }) => {
		requireAdmin(locals);
		return planResult('claude-plan', await claudePlanStatus(), locals.locale);
	},
	removeKey: async ({ locals, request }) => {
		requireAdmin(locals);
		const { provider } = await keyForm(request);
		removeApiKey(provider);
		return { provider, keyMessage: translations(locals.locale).m.admin.removed };
	},
	chatgptSignIn: async ({ locals }) => {
		requireAdmin(locals);
		try {
			// Waits for the code, not for it to be entered: that goes on in the background.
			await startChatGptSignIn();
		} catch (err) {
			if (!(err instanceof PlanError)) throw err;
			return fail(400, { plan: 'chatgpt-plan' as const, planError: err.message });
		}
	},
	chatgptCancel: ({ locals }) => {
		requireAdmin(locals);
		cancelChatGptSignIn();
	},
	chatgptSignOut: async ({ locals }) => {
		requireAdmin(locals);
		try {
			await signOutChatGpt();
		} catch (err) {
			if (!(err instanceof PlanError)) throw err;
			return fail(400, { plan: 'chatgpt-plan' as const, planError: err.message });
		}
		return {
			plan: 'chatgpt-plan' as const,
			planMessage: translations(locals.locale).m.admin.signedOut
		};
	},
	add: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		const provider = form.get('provider')?.toString() ?? '';
		const model = form.get('model')?.toString() ?? '';
		const name = form.get('name')?.toString() ?? '';
		// Left out for the model's own window; otherwise a chip's count, or one typed like "272k".
		const cw = form.get('contextWindow')?.toString().trim() ?? '';
		const contextWindow = cw ? parseTokens(cw) : null;
		const { addModel } = translations(locals.locale).m.admin;
		if (Number.isNaN(contextWindow)) return fail(400, { addError: addModel.invalidContext });
		try {
			const preset = await addPreset({ provider, model, name, contextWindow });
			return { message: addModel.added(preset.name) };
		} catch (err) {
			return fail(400, { addError: err instanceof Error ? err.message : String(err) });
		}
	},
	setDefault: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		try {
			const preset = setDefaultPreset(id);
			return { message: translations(locals.locale).m.admin.newDefault(preset.name) };
		} catch (err) {
			return fail(400, { message: err instanceof Error ? err.message : String(err) });
		}
	},
	remove: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		removePreset(id);
		return { message: translations(locals.locale).m.admin.presetRemoved };
	}
};
