import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, renameSync, rmSync } from 'node:fs';
import { basename, join } from 'node:path';
import { and, asc, eq, inArray, sql } from 'drizzle-orm';
import { AttachmentError, findUploads, saveAttachment, sameContent } from './attachments.ts';
import { getDb } from './db/index.ts';
import { conversation, folder, folderFile, upload } from './db/schema.ts';
import { describeStored } from './media.ts';
import { paths, profileFoldersDir } from './paths.ts';
import type { Profile } from './profiles.ts';

/*
 * Folders group a profile's chats, like projects: every chat in a folder gets the folder's
 * instructions and the paths of its files in its system prompt. Pictures and documents are not
 * sent to the model with the prompt; the agent opens them with commands when they matter, so a
 * folder with many files costs a few lines per chat.
 */

export type Folder = typeof folder.$inferSelect;
export type FolderFile = typeof folderFile.$inferSelect;

export const MAX_FOLDER_NAME = 80;
/** They go into the system prompt of every chat in the folder. */
export const MAX_FOLDER_INSTRUCTIONS = 8000;
export const MAX_FOLDER_FILES = 50;

/** Something wrong with what was asked, in words for the person asking. */
export class FolderError extends Error {}

export function folderDir(profileSlug: string, folderSlug: string): string {
	return join(profileFoldersDir(profileSlug), folderSlug);
}

function cleanName(name: string): string {
	const trimmed = name.replace(/\s+/g, ' ').trim();
	if (!trimmed) throw new FolderError('Give the folder a name.');
	if (trimmed.length > MAX_FOLDER_NAME) {
		throw new FolderError(`Folder names can be at most ${MAX_FOLDER_NAME} characters.`);
	}
	return trimmed;
}

/** Unique in the profile, and not taken by a folder on disk (a deleted folder's, say). */
function slugify(profile: Pick<Profile, 'id' | 'slug'>, name: string): string {
	const base =
		name
			.normalize('NFKD')
			.replace(/[̀-ͯ]/g, '')
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 40) || 'folder';
	const taken = (slug: string) =>
		!!getDb()
			.select({ id: folder.id })
			.from(folder)
			.where(and(eq(folder.profileId, profile.id), eq(folder.slug, slug)))
			.get() || existsSync(folderDir(profile.slug, slug));
	let slug = base;
	for (let i = 2; taken(slug); i++) slug = `${base}-${i}`;
	return slug;
}

export function createFolder(input: {
	profile: Pick<Profile, 'id' | 'slug'>;
	name: string;
	userId: string | null;
	instructions?: string;
}): Folder {
	const name = cleanName(input.name);
	const created: Folder = {
		id: randomUUID(),
		profileId: input.profile.id,
		slug: slugify(input.profile, name),
		name,
		instructions: cleanInstructions(input.instructions ?? ''),
		createdBy: input.userId,
		createdAt: new Date()
	};
	mkdirSync(folderDir(input.profile.slug, created.slug), { recursive: true });
	getDb().insert(folder).values(created).run();
	return created;
}

export function listFolders(profileId: string): Folder[] {
	return getDb()
		.select()
		.from(folder)
		.where(eq(folder.profileId, profileId))
		.orderBy(sql`${folder.name} collate nocase`, asc(folder.createdAt))
		.all();
}

/** The folder, if it belongs to this profile. */
export function getFolder(profileId: string, id: string): Folder | undefined {
	return getDb()
		.select()
		.from(folder)
		.where(and(eq(folder.id, id), eq(folder.profileId, profileId)))
		.get();
}

/** Only the name changes; the folder on disk keeps its slug, so paths in prompts stay valid. */
export function renameFolder(id: string, name: string): void {
	getDb()
		.update(folder)
		.set({ name: cleanName(name) })
		.where(eq(folder.id, id))
		.run();
}

function cleanInstructions(text: string): string {
	const trimmed = text.replace(/\r\n?/g, '\n').trim();
	if (trimmed.length > MAX_FOLDER_INSTRUCTIONS) {
		throw new FolderError(
			`Instructions can be at most ${MAX_FOLDER_INSTRUCTIONS.toLocaleString('en-US')} characters.`
		);
	}
	return trimmed;
}

/** Chats in the folder get the new instructions at the start of their next turn. */
export function setFolderInstructions(id: string, text: string): void {
	getDb()
		.update(folder)
		.set({ instructions: cleanInstructions(text) })
		.where(eq(folder.id, id))
		.run();
}

/** Its chats move back to the profile's list, and its files to ~/.btw-agent/trash. */
export function deleteFolder(
	profile: Pick<Profile, 'slug'>,
	found: Folder
): { trashedTo: string | null } {
	let trashedTo: string | null = null;
	const dir = folderDir(profile.slug, found.slug);
	if (existsSync(dir)) {
		mkdirSync(paths.trash, { recursive: true });
		trashedTo = join(paths.trash, `${profile.slug}-folder-${found.slug}-${Date.now()}`);
		renameSync(dir, trashedTo);
	}
	// conversation.folder_id is set to null by its foreign key.
	getDb().delete(folder).where(eq(folder.id, found.id)).run();
	return { trashedTo };
}

// --- files ---

export function listFolderFiles(folderId: string): FolderFile[] {
	return getDb()
		.select()
		.from(folderFile)
		.where(eq(folderFile.folderId, folderId))
		.orderBy(asc(folderFile.createdAt), asc(folderFile.name))
		.all();
}

export function getFolderFile(folderId: string, id: string): FolderFile | undefined {
	return getDb()
		.select()
		.from(folderFile)
		.where(and(eq(folderFile.id, id), eq(folderFile.folderId, folderId)))
		.get();
}

/**
 * Adds files the person attached on the folder's page (uploads, like the composer's) to the
 * folder: each is copied into the folder's own folder, where the agent finds it, and stays in
 * the media store for the page. A file that's already there isn't added twice.
 */
export async function addFolderFiles(input: {
	profile: Pick<Profile, 'id' | 'slug'>;
	folder: Folder;
	userId: string;
	uploadIds: string[];
}): Promise<FolderFile[]> {
	const { profile, folder: target, userId, uploadIds } = input;
	let uploads;
	try {
		uploads = findUploads(profile.id, userId, uploadIds);
	} catch (err) {
		if (err instanceof AttachmentError) throw new FolderError(err.message);
		throw err;
	}
	const existing = listFolderFiles(target.id);
	if (existing.length + uploads.length > MAX_FOLDER_FILES) {
		throw new FolderError(`A folder can hold at most ${MAX_FOLDER_FILES} files.`);
	}
	const dir = folderDir(profile.slug, target.slug);
	const saved = new Set(existing.map((f) => f.path));
	const rows: FolderFile[] = [];
	for (const up of uploads) {
		const path = saveAttachment(dir, up.name, up.sha256, up.bytes);
		if (saved.has(path)) continue;
		saved.add(path);
		const shown = await describeStored(
			{ sha256: up.sha256, bytes: up.bytes, mime: up.mime },
			up.name,
			path
		);
		rows.push({
			id: randomUUID(),
			folderId: target.id,
			name: basename(path),
			path,
			sha256: up.sha256,
			mime: up.mime,
			bytes: up.bytes,
			width: shown.width,
			height: shown.height,
			previewSha256: shown.previewSha256,
			createdBy: userId,
			createdAt: new Date()
		});
	}
	getDb().transaction((tx) => {
		const taken = tx
			.delete(upload)
			.where(inArray(upload.id, uploadIds))
			.returning({ id: upload.id })
			.all();
		if (taken.length !== uploadIds.length) {
			throw new FolderError('A file was already added. Attach it again.');
		}
		if (rows.length) tx.insert(folderFile).values(rows).run();
	});
	return rows;
}

/** Takes a file out of the folder, and deletes its copy unless someone changed it since. */
export function removeFolderFile(folderId: string, id: string): boolean {
	const row = getFolderFile(folderId, id);
	if (!row) return false;
	getDb().delete(folderFile).where(eq(folderFile.id, id)).run();
	if (sameContent(row.path, row.sha256, row.bytes)) rmSync(row.path, { force: true });
	return true;
}

// --- chats ---

/**
 * Puts a chat in a folder, or back in the profile's list with `null`. Its system prompt is built
 * again at the start of its next turn, which costs one prompt cache miss.
 */
export function moveConversation(
	profileId: string,
	conversationId: string,
	folderId: string | null
): void {
	if (folderId && !getFolder(profileId, folderId)) throw new FolderError('No such folder.');
	getDb()
		.update(conversation)
		.set({ folderId })
		.where(and(eq(conversation.id, conversationId), eq(conversation.profileId, profileId)))
		.run();
}

// --- the system prompt ---

function describeSize(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function describeType(mime: string): string {
	if (mime.startsWith('image/')) return 'picture';
	if (mime === 'application/pdf') return 'PDF';
	return mime;
}

/**
 * The folder's section of a chat's system prompt: its name, instructions and file paths. Chats
 * compare it with the one in their prompt at the start of each turn, so it must only change
 * when the folder does.
 */
export function renderFolderSection(
	profile: Pick<Profile, 'slug'>,
	found: Pick<Folder, 'name' | 'slug' | 'instructions'>,
	files: Pick<FolderFile, 'path' | 'mime' | 'bytes'>[]
): string {
	const dir = folderDir(profile.slug, found.slug);
	const instructions = found.instructions.trim();
	const parts = [
		`# Folder
This conversation is in the folder "${found.name}". The family keeps related conversations in a folder, and every conversation in it shares the folder's instructions and files, which they set on the folder's page.`
	];
	if (instructions) {
		parts.push(`## Instructions for this folder
Follow these in this conversation, as if the family had said them at the start:

${instructions}`);
	}
	if (files.length) {
		const list = files
			.map((f) => `- \`${f.path}\` (${describeType(f.mime)}, ${describeSize(f.bytes)})`)
			.join('\n');
		parts.push(`## Files in this folder
They are saved in \`${dir}\`. Read the ones that matter for a request before answering it: open documents with commands and look at pictures with \`btw view\`.

${list}`);
	} else if (!instructions) {
		parts.push('The folder has no instructions or files yet.');
	}
	return parts.join('\n\n');
}

/** The folder section a chat in this folder should have now; '' outside a folder. */
export function folderContextFor(
	profile: Pick<Profile, 'id' | 'slug'>,
	folderId: string | null
): string {
	if (!folderId) return '';
	const found = getFolder(profile.id, folderId);
	if (!found) return '';
	return renderFolderSection(profile, found, listFolderFiles(found.id));
}
