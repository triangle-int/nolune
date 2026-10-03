import { error, json } from '@sveltejs/kit';
import {
	FolderError,
	MAX_FOLDER_FILES,
	MAX_FOLDER_INSTRUCTIONS,
	deleteFolder,
	getFolder,
	isViewable,
	listFolderFiles,
	renameFolder,
	setFolderInstructions
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

function requireFolder(locals: App.Locals, params: { slug: string; folder: string }) {
	const { profile } = requireProfile(locals, params.slug);
	const found = getFolder(profile.id, params.folder);
	if (!found) error(404, translations(locals.locale).m.errors.folderNotFound);
	return { profile, folder: found };
}

/** A folder as its page shows it: its name, instructions and files (`/files/<id>` each). */
export const GET: RequestHandler = ({ params, locals }) => {
	const { folder } = requireFolder(locals, params);
	return json({
		id: folder.id,
		name: folder.name,
		instructions: folder.instructions,
		files: listFolderFiles(folder.id).map((f) => ({
			id: f.id,
			name: f.name,
			mime: f.mime,
			bytes: f.bytes,
			viewable: isViewable(f)
		})),
		maxFiles: MAX_FOLDER_FILES,
		maxInstructions: MAX_FOLDER_INSTRUCTIONS
	});
};

/**
 * Renames a folder or sets its instructions, as its page does: `{ name }`, `{ instructions }` or
 * both, answering with them as kept.
 */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	const { folder } = requireFolder(locals, params);
	const body = (await request.json().catch(() => null)) as {
		name?: unknown;
		instructions?: unknown;
	} | null;
	const { name, instructions } = body ?? {};
	if (name === undefined && instructions === undefined) error(400, 'Name the folder');
	if (
		(name !== undefined && typeof name !== 'string') ||
		(instructions !== undefined && typeof instructions !== 'string')
	) {
		error(400, 'Send text');
	}
	try {
		if (typeof name === 'string') renameFolder(folder.id, name);
		if (typeof instructions === 'string') setFolderInstructions(folder.id, instructions);
	} catch (err) {
		if (err instanceof FolderError) error(400, err.message);
		throw err;
	}
	const kept = getFolder(folder.profileId, folder.id) ?? folder;
	return json({ id: kept.id, name: kept.name, instructions: kept.instructions });
};

/** Deletes a folder, as its page does: its chats stay, out of any folder, and its files go to the trash. */
export const DELETE: RequestHandler = ({ params, locals }) => {
	const { profile, folder } = requireFolder(locals, params);
	deleteFolder(profile, folder);
	return new Response(null, { status: 204 });
};
