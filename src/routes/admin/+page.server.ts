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
	editPreset,
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
import { translations } from '$lib/i18n';
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
		const status = await claudePlanStatus();
		if (status.problem || !status.account) {
			const { m } = translations(locals.locale);
			return fail(400, { planError: status.problem ?? m.admin.claudeNoAnswer });
		}
		const signedIn = describeAccount(status.account);
		return { planMessage: `${signedIn[0].toUpperCase()}${signedIn.slice(1)}.` };
	},
	removeKey: async ({ locals, request }) => {
		requireAdmin(locals);
		const { provider } = await keyForm(request);
		removeApiKey(provider);
		return { provider, keyMessage: translations(locals.locale).m.admin.removed };
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
	remove: async ({ locals, request }) => {
		requireAdmin(locals);
		const id = (await request.formData()).get('id')?.toString() ?? '';
		removePreset(id);
		return { message: translations(locals.locale).m.admin.presetRemoved };
	}
};
