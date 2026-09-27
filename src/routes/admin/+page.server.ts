import { error, fail } from '@sveltejs/kit';
import {
	CLAUDE_INSTALL_COMMAND,
	CODEX_INSTALL_COMMAND,
	ApiKeyError,
	CustomServerError,
	DEFAULT_EMBEDDING_MODELS,
	PlanError,
	PROVIDERS,
	PROVIDER_LABELS,
	addPreset,
	apiKeyStatuses,
	cancelChatGptSignIn,
	checkApiKey,
	checkServer,
	claudePlanStatus,
	chatGptPlanStatus,
	chatGptSignInState,
	findServer,
	editPreset,
	effectiveContextWindow,
	embeddingProblem,
	embeddingState,
	findClaudeCode,
	findCodex,
	getDefaultPreset,
	isApiKeyProvider,
	isServerName,
	isServerUrl,
	listPresets,
	listProfiles,
	listServers,
	normalizeApiKey,
	normalizeServerUrl,
	removeApiKey,
	removePreset,
	removeServer,
	saveApiKey,
	saveServer,
	splitModel,
	saveEmbeddingSetting,
	setDefaultPreset,
	signOutChatGpt,
	startChatGptSignIn,
	startEmbeddingMemory,
	type EmbeddingSetting,
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
		// The family's servers: each one's address and whether it has a key, never the key.
		servers: listServers(),
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
		// What memory search finds meaning with.
		embeddings: embeddingState(),
		embeddingDefaults: DEFAULT_EMBEDDING_MODELS,
		presets: listPresets().map((p) => ({
			id: p.id,
			name: p.name,
			provider: p.provider,
			model: p.model,
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
	/** Adds a server (`adding`), or changes one; `server` in the result says which row it's for. */
	saveServer: async ({ locals, request }) => {
		requireAdmin(locals);
		const { m } = translations(locals.locale);
		const t = m.admin.servers;
		const form = await request.formData();
		const adding = form.get('adding') === '1';
		const name = form.get('name')?.toString().trim() ?? '';
		const url = normalizeServerUrl(form.get('url')?.toString() ?? '');
		const refuse = (serverError: string) => fail(400, { server: adding ? '' : name, serverError });
		const current = findServer(name);
		if (adding && !isServerName(name)) return refuse(t.needName);
		if (adding && current) return refuse(t.nameTaken(current.name));
		if (!adding && !current) error(400, 'Unknown server');
		if (!isServerUrl(url)) return refuse(t.needAddress);
		const typed = form.get('key')?.toString().trim() || undefined;
		// Left empty, the key saved for the same address stays: the page never has it.
		const key = typed ?? (current?.url === url ? (current.key ?? null) : null);
		let result: { serverMessage: string } | { serverWarning: string };
		try {
			const found = await checkServer(url, key);
			result = found.warning
				? { serverWarning: m.admin.savedWarning(found.warning) }
				: { serverMessage: t.works(found.models.length) };
		} catch (err) {
			if (!(err instanceof CustomServerError)) throw err;
			// Saved all the same: the server may not run yet.
			if (err.reason !== 'unreachable') return refuse(err.message);
			result = { serverWarning: t.unchecked(err.message) };
		}
		saveServer(current?.name ?? name, url, typed);
		// For memory search, when it uses the server.
		startEmbeddingMemory(listProfiles().map((p) => p.slug));
		return { server: current?.name ?? name, ...result };
	},
	removeServer: async ({ locals, request }) => {
		requireAdmin(locals);
		const name = (await request.formData()).get('name')?.toString() ?? '';
		if (!findServer(name)) error(400, 'Unknown server');
		removeServer(name);
		return { server: '', serverMessage: translations(locals.locale).m.admin.removed };
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
			// `<server>/<model>`, as the form puts it together.
			const on = splitModel(model);
			if (!on.server || !findServer(on.server)) error(400, 'Unknown server');
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
