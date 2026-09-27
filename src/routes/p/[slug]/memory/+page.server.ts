import { fail } from '@sveltejs/kit';
import {
	CORE_NOTE,
	MAX_PINNED_CHARS,
	MemoryConflictError,
	MemoryError,
	forgetMemoryFile,
	listMemoryFiles,
	writeMemoryFile
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	return {
		files: listMemoryFiles(profile.slug),
		core: { path: CORE_NOTE, maxChars: MAX_PINNED_CHARS }
	};
};

function message(err: unknown): string {
	if (err instanceof MemoryError || err instanceof MemoryConflictError) return err.message;
	throw err;
}

export const actions: Actions = {
	save: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const path = form.get('path')?.toString() ?? '';
		const text = form.get('text')?.toString() ?? '';
		if (!text.trim()) {
			return fail(400, { path, message: 'The note is empty. To delete it, use Forget.' });
		}
		try {
			writeMemoryFile(profile.slug, path, text, Number(form.get('basedOn')));
		} catch (err) {
			if (err instanceof MemoryConflictError) {
				return fail(409, {
					path,
					conflict: true,
					message: `${message(err)} Save again to keep your version, or cancel to see btw's.`
				});
			}
			return fail(400, { path, message: message(err) });
		}
		return { path, message: 'Saved. New chats will see the change.' };
	},
	forget: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const path = (await request.formData()).get('path')?.toString() ?? '';
		try {
			forgetMemoryFile(profile.slug, path);
		} catch (err) {
			return fail(400, { path, message: message(err) });
		}
		return { message: `btw forgot everything in ${path}.` };
	}
};
