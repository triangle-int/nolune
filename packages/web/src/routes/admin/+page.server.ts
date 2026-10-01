import { error, fail } from '@sveltejs/kit';
import {
	CHATGPT_USAGE_URL,
	CLAUDE_INSTALL_COMMAND,
	ApiKeyError,
	CustomProviderError,
	McpServerError,
	DEFAULT_EMBEDDING_MODELS,
	PlanError,
	PROVIDERS,
	PROVIDER_LABELS,
	addPreset,
	apiKeyStatuses,
	cancelChatGptSignIn,
	checkApiKey,
	checkCustomProvider,
	checkMcpServer,
	claudePlanStatus,
	chatGptPlanStatus,
	commandSafetyState,
	chatGptSignInState,
	findCustomProvider,
	customProviderNameProblem,
	describeFromServer,
	editPreset,
	effectiveContextWindow,
	embeddingProblem,
	embeddingState,
	findClaudeCode,
	findMcpServer,
	finishChatGptSignIn,
	getDefaultPreset,
	getPreset,
	isApiKeyProvider,
	isCommandMode,
	isCustomProvider,
	isMcpAddress,
	isProviderUrl,
	listMcpServers,
	listPresets,
	listProfiles,
	listCustomProviders,
	mcpServerNameProblem,
	parseMcpServer,
	normalizeApiKey,
	normalizeProviderUrl,
	removeApiKey,
	removeMcpServer,
	removePreset,
	removeCustomProvider,
	saveApiKey,
	saveCommandMode,
	saveCustomProvider,
	saveMcpServer,
	saveSafetyPreset,
	splitCommandLine,
	splitModel,
	saveEmbeddingSetting,
	setDefaultPreset,
	signOutChatGpt,
	startChatGptSignIn,
	startEmbeddingMemory,
	type CustomApi,
	type EmbeddingSetting,
	type McpServerConfig,
	type McpServerTools,
	type Plan,
	type PlanStatus
} from '@nolune/core';
import { parseTokens } from '$lib/format';
import { translations } from '$lib/i18n';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, depends }) => {
	requireAdmin(locals);
	// The page asks again while a ChatGPT sign-in waits for the browser to come back.
	depends('nolune:chatgpt-plan');
	const defaultId = getDefaultPreset()?.id;
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
		// Who's signed in with ChatGPT, from what nolune keeps (asking OpenAI is Check sign-in's),
		// and a sign-in under way. Never the tokens.
		chatgpt: {
			...signIn,
			usageUrl: CHATGPT_USAGE_URL,
			status: await chatGptPlanStatus()
		},
		// What memory search finds meaning with.
		embeddings: embeddingState(),
		embeddingDefaults: DEFAULT_EMBEDDING_MODELS,
		// Whether a model checks the agent's commands first, and which.
		commands: commandSafetyState(),
		// MCP servers: each one's command or address and the names of its keys, never the keys.
		mcpServers: listMcpServers(),
		profiles: listProfiles().map((p) => ({ slug: p.slug, name: p.name })),
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
		return fail(400, {
			plan,
			planError:
				status.problem ?? (plan === 'claude-plan' ? m.admin.claudeNoAnswer : m.admin.chatgptNobody)
		});
	}
	const { signedIn } = status;
	return { plan, planMessage: `${signedIn[0].toUpperCase()}${signedIn.slice(1)}.` };
}

/** A message from core at the start of a sentence. */
function sentence(text: string): string {
	return `${text[0]?.toUpperCase() ?? ''}${text.slice(1)}`;
}

/** An MCP server form's keys: a line each, `NAME=value` or `Name: value`; the first bad line. */
function secretLines(
	text: string,
	separator: '=' | ':'
): { map: Record<string, string> } | { bad: string } {
	const map: Record<string, string> = {};
	for (const line of text.split('\n').map((l) => l.trim())) {
		if (!line) continue;
		const at = line.indexOf(separator);
		if (at <= 0) return { bad: line.length > 40 ? `${line.slice(0, 40)}…` : line };
		map[line.slice(0, at).trim()] = line.slice(at + 1).trim();
	}
	return { map };
}

/** A few of a server's tools, by name, for a sentence. */
function someTools(found: McpServerTools): string {
	const names = found.tools.map((t) => t.name);
	return names.length > 6 ? `${names.slice(0, 6).join(', ')}…` : names.join(', ');
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
	/**
	 * Connects an MCP server (no `editing`), or changes the one `editing` names, after connecting
	 * to it to check it; one it can't connect to is saved with why. `mcpServer` in the result says
	 * which row it's for, or the add form with `''`.
	 */
	saveMcpServer: async ({ locals, request }) => {
		requireAdmin(locals);
		const t = translations(locals.locale).m.admin.mcp;
		const form = await request.formData();
		const editing = form.get('editing')?.toString() || undefined;
		const name = editing ?? form.get('name')?.toString().trim() ?? '';
		const refuse = (mcpError: string) => fail(400, { mcpServer: editing ?? '', mcpError });
		const current = editing ? listMcpServers().find((s) => s.name === editing) : undefined;
		if (editing && !current) error(400, 'Unknown MCP server');
		if (!editing) {
			const problem = mcpServerNameProblem(name);
			if (problem) return refuse(problem);
			if (listMcpServers().some((s) => s.name === name)) return refuse(t.taken(name));
		}
		const slugs = new Set(listProfiles().map((p) => p.slug));
		const profiles = form.getAll('profiles').map(String);
		if (profiles.some((slug) => !slugs.has(slug))) error(400, 'Unknown profile');
		const description = form.get('description')?.toString().trim() || undefined;
		const secrets = form.get('secrets')?.toString().trim() ?? '';
		// The saved one, keys and all: a form that leaves its keys empty keeps them.
		let saved: McpServerConfig | null = null;
		try {
			saved = editing ? findMcpServer(editing) : null;
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
		}
		const kind = form.get('kind')?.toString();
		let server: McpServerConfig;
		if (kind === 'stdio') {
			let words: string[];
			try {
				words = splitCommandLine(form.get('command')?.toString() ?? '');
			} catch (err) {
				if (!(err instanceof McpServerError)) throw err;
				return refuse(err.message);
			}
			if (!words.length) return refuse(t.needCommand);
			const env = secretLines(secrets, '=');
			if ('bad' in env) return refuse(t.badEnv(env.bad));
			const kept = saved?.type === 'stdio' ? saved : null;
			server = {
				type: 'stdio',
				command: words[0],
				args: words.slice(1),
				env: secrets ? env.map : kept?.env,
				// Only the CLI sets where it starts; it stays.
				cwd: kept?.cwd,
				description,
				profiles
			};
		} else if (kind === 'remote') {
			const url = form.get('url')?.toString().trim() ?? '';
			if (!isMcpAddress(url)) return refuse(t.needAddress);
			const headers = secretLines(secrets, ':');
			if ('bad' in headers) return refuse(t.badHeader(headers.bad));
			const kept = saved && saved.type !== 'stdio' ? saved : null;
			server = {
				type: form.get('transport')?.toString() === 'sse' ? 'sse' : 'http',
				url,
				headers: secrets ? headers.map : kept?.headers,
				description,
				profiles
			};
		} else error(400, 'Unknown kind of MCP server');
		let parsed: McpServerConfig;
		try {
			parsed = parseMcpServer(server);
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
			return refuse(err.message);
		}
		let found: McpServerTools | null = null;
		let problem = '';
		try {
			found = await checkMcpServer(name, parsed);
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
			problem = err.message;
		}
		if (found && !parsed.description) parsed.description = describeFromServer(found);
		saveMcpServer(name, parsed);
		return found
			? { mcpServer: name, mcpMessage: t.works(found.tools.length, someTools(found)) }
			: { mcpServer: name, mcpWarning: t.unchecked(sentence(problem)) };
	},
	/** Connects to a saved MCP server, to see that it works. */
	checkMcpServer: async ({ locals, request }) => {
		requireAdmin(locals);
		const t = translations(locals.locale).m.admin.mcp;
		const name = (await request.formData()).get('name')?.toString() ?? '';
		if (!listMcpServers().some((s) => s.name === name)) error(400, 'Unknown MCP server');
		try {
			const found = await checkMcpServer(name, findMcpServer(name));
			return { mcpServer: name, mcpMessage: t.works(found.tools.length, someTools(found)) };
		} catch (err) {
			if (!(err instanceof McpServerError)) throw err;
			return fail(400, { mcpServer: name, mcpError: sentence(err.message) });
		}
	},
	removeMcpServer: async ({ locals, request }) => {
		requireAdmin(locals);
		const name = (await request.formData()).get('name')?.toString() ?? '';
		if (!listMcpServers().some((s) => s.name === name)) error(400, 'Unknown MCP server');
		removeMcpServer(name);
		return { mcpServer: '', mcpMessage: translations(locals.locale).m.admin.mcp.removed };
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
	chatgptSignIn: async ({ locals, request }) => {
		requireAdmin(locals);
		const anotherAccount = (await request.formData()).get('account')?.toString() === 'another';
		try {
			// Waits for the browser to come back in the background.
			await startChatGptSignIn({ anotherAccount });
		} catch (err) {
			if (!(err instanceof PlanError)) throw err;
			return fail(400, { plan: 'chatgpt-plan' as const, planError: err.message });
		}
	},
	/** A sign-in finished in a browser on another device: the address it ended on. */
	chatgptFinish: async ({ locals, request }) => {
		requireAdmin(locals);
		const address = (await request.formData()).get('address')?.toString() ?? '';
		try {
			await finishChatGptSignIn(address);
		} catch (err) {
			if (!(err instanceof PlanError)) throw err;
			return fail(400, { plan: 'chatgpt-plan' as const, planError: err.message });
		}
		return {
			plan: 'chatgpt-plan' as const,
			planMessage: translations(locals.locale).m.admin.chatgptSignedIn
		};
	},
	chatgptCheck: async ({ locals }) => {
		requireAdmin(locals);
		return planResult('chatgpt-plan', await chatGptPlanStatus({ check: true }), locals.locale);
	},
	chatgptCancel: ({ locals }) => {
		requireAdmin(locals);
		cancelChatGptSignIn();
	},
	chatgptSignOut: async ({ locals }) => {
		requireAdmin(locals);
		const { m } = translations(locals.locale);
		let told: boolean;
		try {
			told = await signOutChatGpt();
		} catch (err) {
			if (!(err instanceof PlanError)) throw err;
			return fail(400, { plan: 'chatgpt-plan' as const, planError: err.message });
		}
		return {
			plan: 'chatgpt-plan' as const,
			planMessage: told ? m.admin.signedOut : m.admin.chatgptSignedOutLocally
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
	commandSafety: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		const mode = form.get('mode')?.toString() ?? '';
		if (!isCommandMode(mode)) error(400, 'Unknown command mode');
		// Empty: each chat's own model.
		const presetId = form.get('presetId')?.toString() || null;
		if (presetId && !getPreset(presetId)) error(400, 'Unknown model preset');
		saveCommandMode(mode);
		// Unrestricted has nothing to check with, and the choice stays for when auto comes back.
		if (mode === 'auto') saveSafetyPreset(presetId);
		return { commandsMessage: translations(locals.locale).m.admin.commands.saved };
	},
	remove: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		removePreset(id);
		return { message: translations(locals.locale).m.admin.presetRemoved };
	}
};
