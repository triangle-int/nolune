import { fail } from '@sveltejs/kit';
import {
	MemoryConflictError,
	MemoryError,
	forgetMemoryFile,
	mergeProfileNotes,
	setLearnFromChats,
	writeMemoryFile
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import { memoryOverview } from '$lib/server/memory';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	return memoryOverview(user, profile);
};

function message(err: unknown): string {
	if (err instanceof MemoryError || err instanceof MemoryConflictError) return err.message;
	throw err;
}

export const actions: Actions = {
	save: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const path = form.get('path')?.toString() ?? '';
		const text = form.get('text')?.toString() ?? '';
		if (!text.trim()) return fail(400, { path, message: m.memory.empty });
		try {
			writeMemoryFile(profile.slug, path, text, Number(form.get('basedOn')));
		} catch (err) {
			if (err instanceof MemoryConflictError) {
				return fail(409, {
					path,
					conflict: true,
					message: m.memory.conflict(message(err))
				});
			}
			return fail(400, { path, message: message(err) });
		}
		return { path, message: m.memory.saved };
	},
	learn: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		setLearnFromChats(profile.id, (await request.formData()).get('on') === 'on');
		// The switch shows the change; no message needed.
		return {};
	},
	move: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const from = form.get('from')?.toString() ?? '';
		const to = form.get('to')?.toString() ?? '';
		try {
			// Into a note that's there already, it's a merge.
			const merged = mergeProfileNotes(profile, from, to);
			return {
				message: (merged.merged ? m.memory.move.merged : m.memory.move.moved)(
					merged.from,
					merged.into
				)
			};
		} catch (err) {
			return fail(400, { message: message(err) });
		}
	},
	forget: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const path = (await request.formData()).get('path')?.toString() ?? '';
		try {
			forgetMemoryFile(profile.slug, path);
		} catch (err) {
			return fail(400, { path, message: message(err) });
		}
		return { message: translations(locals.locale).m.memory.forgot(path) };
	}
};
