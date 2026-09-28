import { error, fail } from '@sveltejs/kit';
import {
	CLAUDE_INSTALL_COMMAND,
	CODEX_INSTALL_COMMAND,
	ApiKeyError,
	CustomProviderError,
	DEFAULT_EMBEDDING_MODELS,
	PlanError,
	PROVIDERS,
	PROVIDER_LABELS,
	addPreset,
	apiKeyStatuses,
	cancelChatGptSignIn,
	checkApiKey,
	checkCustomProvider,
	claudePlanStatus,
	chatGptPlanStatus,
	chatGptSignInState,
	findCustomProvider,
	customProviderNameProblem,
	editPreset,
	effectiveContextWindow,
	embeddingProblem,
	embeddingState,
	findClaudeCode,
	findCodex,
	getDefaultPreset,
	isApiKeyProvider,
	isCustomProvider,
	isProviderUrl,
	listPresets,
	listProfiles,
	listCustomProviders,
	normalizeApiKey,
	normalizeProviderUrl,
	removeApiKey,
	removePreset,
	removeCustomProvider,
	saveApiKey,
	saveCustomProvider,
	splitModel,
	saveEmbeddingSetting,
	setDefaultPreset,
	signOutChatGpt,
	startChatGptSignIn,
	startEmbeddingMemory,
	type CustomApi,
	type EmbeddingSetting,
	type Plan,
	type PlanStatus
} from '@nolune/core';
import { parseTokens } from '$lib/format';
import { translations } from '$lib/i18n';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, depends }) => {
	requireAdmin(locals);
	// The page asks again while a ChatGPT sign-in waits for its code.
	depends('nolune:chatgpt-plan');
	const defaultId = getDefaultPreset()?.id;
	const codex = findCodex();
	const signIn = chatGptSignInState();
	return {
		// Where each key comes from and its last four characters; never the keys themselves.
		keys: apiKeyStatuses(),
		// Custom providers: each one's API, address and whether it has a key, never the key.
		customProviders: listCustomProviders(),
		// Custom providers are chips of their own.
		providers: PROVIDERS.filter((id) => !isCustomProvider(id)).map((id) => ({
			id,
			label: PROVIDER_LABELS[id]
		})),
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
		// What memory search finds meaning with.
		embeddings: embeddingState(),
		embeddingDefaults: DEFAULT_EMBEDDING_MODELS,
		presets: listPresets().map((p) => ({
			id: p.id,
			name: p.name,
			provider: p.provider,
			model: p.model,
			...shownAs(p.provider, p.model),
			contextWindow: effectiveContextWindow(p),
			/** The admin's own window, which Auto leaves out. */
			override: p.contextWindow,
			isDefault: p.id === defaultId
		}))
	};
};

/**
 * A preset form's fields. The whole preset is sent, so an empty name is the default one and a
 * left-out context window is the model's own (null); NaN when the one typed isn't a count.
 */
function presetFields(form: FormData) {
	const cw = form.get('contextWindow')?.toString().trim() ?? '';
	return {
		provider: form.get('provider')?.toString() ?? '',
		model: form.get('model')?.toString() ?? '',
		name: form.get('name')?.toString() ?? '',
		// A chip's count, or one typed like "272k".
		contextWindow: cw ? parseTokens(cw) : null
	};
}

/** How a preset's provider and model show: a custom provider's by its name, its model bare. */
function shownAs(provider: string, model: string) {
	const on = isCustomProvider(provider) ? splitModel(model) : null;
	const custom = on?.provider ? findCustomProvider(on.provider) : undefined;
	return custom && on
		? { shownProvider: custom.name, shownModel: on.model }
		: { shownProvider: provider, shownModel: model };
}

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
	/**
	 * Adds a custom provider (no `id`), or changes one; `customProvider` in the result says which
	 * row it's for, or the add form with `''`.
	 */
	saveCustomProvider: async ({ locals, request }) => {
		requireAdmin(locals);
		const { m } = translations(locals.locale);
		const t = m.admin.customProviders;
		const form = await request.formData();
		const id = form.get('id')?.toString() || undefined;
		const name = form.get('name')?.toString() ?? '';
		const api = form.get('api')?.toString();
		const url = normalizeProviderUrl(form.get('url')?.toString() ?? '');
		const refuse = (customError: string) => fail(400, { customProvider: id ?? '', customError });
		const current = id ? findCustomProvider(id) : undefined;
		if (id && current?.id !== id) error(400, 'Unknown custom provider');
		if (!id && api !== 'openai' && api !== 'anthropic') error(400, 'Unknown API');
		if (!name.trim()) return refuse(t.needName);
		const problem = customProviderNameProblem(name, id);
		if (problem) return refuse(problem);
		if (!isProviderUrl(url)) return refuse(t.needAddress);
		const typed = form.get('key')?.toString().trim() || undefined;
		// Left empty, the key saved for the same address stays: the page never has it.
		const key = typed ?? (current?.url === url ? (current.key ?? null) : null);
		let result: { customMessage: string } | { customWarning: string };
		try {
			const found = await checkCustomProvider(url, key);
			result = found.warning
				? { customWarning: m.admin.savedWarning(found.warning) }
				: { customMessage: t.works(found.models.length) };
		} catch (err) {
			if (!(err instanceof CustomProviderError)) throw err;
			// Saved all the same: its server may not run yet.
			if (err.reason !== 'unreachable') return refuse(err.message);
			result = { customWarning: t.unchecked(err.message) };
		}
		let saved: string;
		try {
			saved = saveCustomProvider({ id, name, api: api as CustomApi, url, key: typed });
		} catch (err) {
			if (!(err instanceof CustomProviderError)) throw err;
			return refuse(err.message);
		}
		// For memory search, when it uses this one.
		startEmbeddingMemory(listProfiles().map((p) => p.slug));
		return { customProvider: saved, ...result };
	},
	removeCustomProvider: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		if (findCustomProvider(id)?.id !== id) error(400, 'Unknown custom provider');
		removeCustomProvider(id);
		return { customProvider: '', customMessage: translations(locals.locale).m.admin.removed };
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
		const { addModel } = translations(locals.locale).m.admin;
		const fields = presetFields(await request.formData());
		if (Number.isNaN(fields.contextWindow)) return fail(400, { addError: addModel.invalidContext });
		try {
			const preset = await addPreset(fields);
			return { message: addModel.added(preset.name) };
		} catch (err) {
			return fail(400, { addError: err instanceof Error ? err.message : String(err) });
		}
	},
	edit: async ({ locals, request }) => {
		requireAdmin(locals);
		const { addModel } = translations(locals.locale).m.admin;
		const form = await request.formData();
		const editId = form.get('id')?.toString() ?? '';
		const fields = presetFields(form);
		if (Number.isNaN(fields.contextWindow)) {
			return fail(400, { editId, editError: addModel.invalidContext });
		}
		try {
			const preset = await editPreset(editId, fields);
			return { message: addModel.saved(preset.name) };
		} catch (err) {
			return fail(400, { editId, editError: err instanceof Error ? err.message : String(err) });
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
	embeddings: async ({ locals, request }) => {
		requireAdmin(locals);
		const t = translations(locals.locale).m.admin.embeddings;
		const form = await request.formData();
		const mode = form.get('mode')?.toString() ?? '';
		const model = form.get('model')?.toString().trim() ?? '';
		let setting: EmbeddingSetting | undefined;
		if (mode === 'openai' || mode === 'openrouter') {
			setting = { provider: mode, model: model || DEFAULT_EMBEDDING_MODELS[mode] };
		} else if (mode === 'custom-openai') {
			// `<id>/<model>`, as the form puts it together.
			const on = splitModel(model);
			if (findCustomProvider(on.provider)?.api !== 'openai') error(400, 'Unknown custom provider');
			if (!on.model) return fail(400, { embeddingsError: t.needModel });
			setting = { provider: mode, model };
		} else if (mode === 'off') {
			setting = 'off';
		} else if (mode !== 'auto') {
			error(400, 'Unknown source');
		}
		saveEmbeddingSetting(setting);
		const problem = await embeddingProblem();
		if (problem) return { embeddingsWarning: t.noAnswer(problem) };
		// Facts the new source hasn't embedded yet are, in the background.
		startEmbeddingMemory(listProfiles().map((p) => p.slug));
		return { embeddingsMessage: embeddingState().using ? t.works : t.wordsOnly };
	},
	remove: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		removePreset(id);
		return { message: translations(locals.locale).m.admin.presetRemoved };
	}
};
