import { fail } from '@sveltejs/kit';
import {
	addPreset,
	effectiveContextWindow,
	getDefaultPreset,
	listPresets,
	removePreset,
	setDefaultPreset
} from '@btw/core';
import { requireAdmin } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals }) => {
	requireAdmin(locals);
	const defaultId = getDefaultPreset()?.id;
	return {
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

export const actions: Actions = {
	add: async ({ locals, request }) => {
		requireAdmin(locals);
		const form = await request.formData();
		const model = form.get('model')?.toString() ?? '';
		const name = form.get('name')?.toString() ?? '';
		const cw = form.get('contextWindow')?.toString().trim() ?? '';
		try {
			await addPreset({ model, name, contextWindow: cw ? Number(cw) : null });
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
