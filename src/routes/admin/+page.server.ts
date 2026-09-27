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
function planResult(plan: Plan, status: PlanStatus) {
	if (status.problem || !status.signedIn) {
		return fail(400, { plan, planError: status.problem ?? "The plan didn't answer." });
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
			return { provider, keyMessage: warning ? `Saved. ${warning}` : 'Saved. It works.' };
		} catch (err) {
			if (!(err instanceof ApiKeyError)) throw err;
			return fail(400, { provider, keyError: err.message });
		}
	},
	checkPlan: async ({ locals }) => {
		requireAdmin(locals);
		return planResult('claude-plan', await claudePlanStatus());
	},
	removeKey: async ({ locals, request }) => {
		requireAdmin(locals);
		const { provider } = await keyForm(request);
		removeApiKey(provider);
		return { provider, keyMessage: 'Removed.' };
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
		return { plan: 'chatgpt-plan' as const, planMessage: 'Signed out.' };
	},
	add: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		const provider = form.get('provider')?.toString() ?? '';
		const model = form.get('model')?.toString() ?? '';
		const name = form.get('name')?.toString() ?? '';
		const cw = form.get('contextWindow')?.toString().trim() ?? '';
		try {
			await addPreset({ provider, model, name, contextWindow: cw ? Number(cw) : null });
		} catch (err) {
			return fail(400, { message: err instanceof Error ? err.message : String(err) });
		}
		return { message: 'Added.' };
	},
	setDefault: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		try {
			const preset = setDefaultPreset(id);
			return { message: `New chats now start with ${preset.name}.` };
		} catch (err) {
			return fail(400, { message: err instanceof Error ? err.message : String(err) });
		}
	},
	remove: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		removePreset(id);
		return { message: 'Removed. Existing conversations keep working.' };
	}
};
