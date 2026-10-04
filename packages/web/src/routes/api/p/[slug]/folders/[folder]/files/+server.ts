import { error, json } from '@sveltejs/kit';
import { FolderError, addFolderFiles, getFolder, isViewable, listFolderFiles } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * Adds files to a folder, uploaded first as the composer's are (`/api/p/<slug>/uploads`):
 * `{ uploads: [<id>…] }`. Answers with the folder's files.
 */
export const POST: RequestHandler = async ({ params, locals, request }) => {
	const { user, profile } = requireProfile(locals, params.slug);
	const folder = getFolder(profile.id, params.folder);
	if (!folder) error(404, translations(locals.locale).m.errors.folderNotFound);
	const body = (await request.json().catch(() => null)) as { uploads?: unknown } | null;
	const uploads = body?.uploads;
	if (!Array.isArray(uploads) || !uploads.every((id) => typeof id === 'string')) {
		error(400, 'Send the uploads to add');
	}
	try {
		await addFolderFiles({ profile, folder, userId: user.id, uploadIds: uploads });
	} catch (err) {
		if (err instanceof FolderError) error(400, err.message);
		throw err;
	}
	return json({
		files: listFolderFiles(folder.id).map((f) => ({
			id: f.id,
			name: f.name,
			mime: f.mime,
			bytes: f.bytes,
			viewable: isViewable(f)
		}))
	});
};
