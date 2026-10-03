import { error, json } from '@sveltejs/kit';
import { FolderError, deleteFolder, getFolder, renameFolder } from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

function requireFolder(locals: App.Locals, params: { slug: string; folder: string }) {
	const { profile } = requireProfile(locals, params.slug);
	const found = getFolder(profile.id, params.folder);
	if (!found) error(404, translations(locals.locale).m.errors.folderNotFound);
	return { profile, folder: found };
}

/** Renames a folder, as its page does: `{ name }`, answering with the name as kept. */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	const { folder } = requireFolder(locals, params);
	const body = (await request.json().catch(() => null)) as { name?: unknown } | null;
	if (typeof body?.name !== 'string') error(400, 'Name the folder');
	try {
		renameFolder(folder.id, body.name);
	} catch (err) {
		if (err instanceof FolderError) error(400, err.message);
		throw err;
	}
	return json({ id: folder.id, name: getFolder(folder.profileId, folder.id)?.name ?? body.name });
};

/** Deletes a folder, as its page does: its chats stay, out of any folder, and its files go to the trash. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { profile, folder } = requireFolder(locals, params);
	deleteFolder(profile, folder);
	return new Response(null, { status: 204 });
};
