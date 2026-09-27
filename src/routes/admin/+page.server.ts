import { error, fail } from '@sveltejs/kit';
import {
	CLAUDE_INSTALL_COMMAND,
	ApiKeyError,
	PROVIDERS,
	PROVIDER_LABELS,
	addPreset,
	apiKeyStatuses,
	checkApiKey,
	claudePlanStatus,
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
	setDefaultPreset
} from '@btw/core';
import { parseTokens } from '$lib/format';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	requireAdmin(locals);
	const defaultId = getDefaultPreset()?.id;
	return {
		// Where each key comes from and its last four characters; never the keys themselves.
		keys: apiKeyStatuses(),
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
	add: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		const provider = form.get('provider')?.toString() ?? '';
		const model = form.get('model')?.toString() ?? '';
		const name = form.get('name')?.toString() ?? '';
		// Left out for the model's own window; otherwise a chip's count, or one typed like "272k".
		const cw = form.get('contextWindow')?.toString().trim() ?? '';
		const contextWindow = cw ? parseTokens(cw) : null;
		if (Number.isNaN(contextWindow)) {
			return fail(400, { addError: 'Context window must be a token count, like 272k or 272000.' });
		}
		try {
			const preset = await addPreset({ provider, model, name, contextWindow });
			return { message: `Added ${preset.name}.` };
		} catch (err) {
			return fail(400, { addError: err instanceof Error ? err.message : String(err) });
		}
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
