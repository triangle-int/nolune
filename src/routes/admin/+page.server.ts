import { error, fail } from '@sveltejs/kit';
import {
	CLAUDE_INSTALL_COMMAND,
	ApiKeyError,
	CodexAuthError,
	PROVIDERS,
	PROVIDER_LABELS,
	addPreset,
	apiKeyStatuses,
	cancelCodexSignIn,
	checkApiKey,
	claudePlanStatus,
	codexStatus,
	describeAccount,
	effectiveContextWindow,
	findClaudeCode,
	getDefaultPreset,
	isApiKeyProvider,
	listPresets,
	normalizeApiKey,
	removeApiKey,
	removePreset,
	saveApiKey,
	setDefaultPreset,
	signOutCodex,
	startCodexSignIn
} from '@btw/core';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, depends }) => {
	requireAdmin(locals);
	// The page asks again while a ChatGPT sign-in waits for its code.
	depends('btw:codex');
	const defaultId = getDefaultPreset()?.id;
	return {
		// Where each key comes from and its last four characters; never the keys themselves.
		keys: apiKeyStatuses(),
		// Who btw is signed in to ChatGPT as, and a sign-in's code; never the tokens.
		codex: codexStatus(),
		providers: PROVIDERS.map((id) => ({ id, label: PROVIDER_LABELS[id] })),
		// Where Claude Code is; whether it's signed in takes starting it, so that's a button.
		claude: { ...findClaudeCode(), installCommand: CLAUDE_INSTALL_COMMAND },
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
		const status = await claudePlanStatus();
		if (status.problem || !status.account) {
			return fail(400, { planError: status.problem ?? "Claude Code didn't answer." });
		}
		const signedIn = describeAccount(status.account);
		return { planMessage: `${signedIn[0].toUpperCase()}${signedIn.slice(1)}.` };
	},
	removeKey: async ({ locals, request }) => {
		requireAdmin(locals);
		const { provider } = await keyForm(request);
		removeApiKey(provider);
		return { provider, keyMessage: 'Removed.' };
	},
	codexSignIn: async ({ locals }) => {
		requireAdmin(locals);
		try {
			// Waits for the code, not for it to be entered: that goes on in the background.
			await startCodexSignIn();
		} catch (err) {
			if (!(err instanceof CodexAuthError)) throw err;
			return fail(400, { codexError: err.message });
		}
	},
	codexCancel: ({ locals }) => {
		requireAdmin(locals);
		cancelCodexSignIn();
	},
	codexSignOut: async ({ locals }) => {
		requireAdmin(locals);
		await signOutCodex();
		return { codexMessage: 'Signed out.' };
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
