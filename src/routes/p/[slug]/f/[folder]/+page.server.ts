import { error, fail, redirect } from '@sveltejs/kit';
import {
	EFFORTS,
	FolderError,
	MAX_FOLDER_FILES,
	MAX_FOLDER_INSTRUCTIONS,
	addFolderFiles,
	deleteFolder,
	folderDir,
	getDefaultPreset,
	getFolder,
	isViewable,
	listFolderFiles,
	listPresets,
	removeFolderFile,
	renameFolder,
	setFolderInstructions
} from '@btw/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

function requireFolder(locals: App.Locals, params: { slug: string; folder: string }) {
	const { user, profile } = requireProfile(locals, params.slug);
	const found = getFolder(profile.id, params.folder);
	if (!found) error(404, translations(locals.locale).m.errors.folderNotFound);
	return { user, profile, folder: found };
}

/** What went wrong, for the person on the page; anything else is a real error. */
function failed(err: unknown) {
	if (err instanceof FolderError) return fail(400, { message: err.message });
	throw err;
}

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile, folder } = requireFolder(locals, params);
	return {
		folder: {
			id: folder.id,
			name: folder.name,
			instructions: folder.instructions,
			dir: folderDir(profile.slug, folder.slug)
		},
		files: listFolderFiles(folder.id).map((f) => ({
			id: f.id,
			name: f.name,
			mime: f.mime,
			bytes: f.bytes,
			viewable: isViewable(f)
		})),
		maxFiles: MAX_FOLDER_FILES,
		maxInstructions: MAX_FOLDER_INSTRUCTIONS,
		presets: listPresets().map((p) => ({ id: p.id, name: p.name })),
		defaultPresetId: getDefaultPreset()?.id ?? '',
		efforts: [...EFFORTS]
	};
};

export const actions: Actions = {
	rename: async ({ locals, params, request }) => {
		const { folder } = requireFolder(locals, params);
		const form = await request.formData();
		try {
			renameFolder(folder.id, form.get('name')?.toString() ?? '');
		} catch (err) {
			return failed(err);
		}
	},

	instructions: async ({ locals, params, request }) => {
		const { folder } = requireFolder(locals, params);
		const form = await request.formData();
		try {
			setFolderInstructions(folder.id, form.get('instructions')?.toString() ?? '');
		} catch (err) {
			return failed(err);
		}
	},

	/** Files were uploaded already, like the composer's; this adds them by their upload ids. */
	addFiles: async ({ locals, params, request }) => {
		const { user, profile, folder } = requireFolder(locals, params);
		const form = await request.formData();
		try {
			await addFolderFiles({
				profile,
				folder,
				userId: user.id,
				uploadIds: form.getAll('upload').map(String)
			});
		} catch (err) {
			return failed(err);
		}
	},

	removeFile: async ({ locals, params, request }) => {
		const { folder } = requireFolder(locals, params);
		const form = await request.formData();
		removeFolderFile(folder.id, form.get('file')?.toString() ?? '');
	},

	delete: async ({ locals, params }) => {
		const { profile, folder } = requireFolder(locals, params);
		deleteFolder(profile, folder);
		redirect(303, `/p/${profile.slug}`);
	}
};
