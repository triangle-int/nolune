import { error, fail } from '@sveltejs/kit';
import {
	API_KEYS,
	ApiKeyError,
	PROVIDERS,
	addPreset,
	apiKeyStatuses,
	checkApiKey,
	effectiveContextWindow,
	getDefaultPreset,
	isApiKeyProvider,
	listPresets,
	normalizeApiKey,
	removeApiKey,
	removePreset,
	saveApiKey,
	setDefaultPreset
} from '@btw/core';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	requireAdmin(locals);
	const defaultId = getDefaultPreset()?.id;
	return {
		// Where each key comes from and its last four characters; never the keys themselves.
		keys: apiKeyStatuses(),
		providers: PROVIDERS.map((id) => ({ id, label: API_KEYS[id].label })),
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
