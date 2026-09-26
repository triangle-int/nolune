import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { basename, extname, join } from 'node:path';
import { copyFile, type PreparedMedia } from './media.ts';
import { profileDir } from './paths.ts';

/*
 * Pictures people attach to a message, from the Images page. Each is saved in the profile folder
 * (`uploads/<date>/`), where the agent can use it like any other file, and copied into the media
 * store so the chat shows the message's picture even if the agent moves or edits the file.
 */

export const MAX_UPLOAD_BYTES = 25 * 1024 * 1024;
export const MAX_UPLOADS = 4;

export interface Attachment {
	/** Absolute path of the saved file; the model sees it with the message. */
	path: string;
	/** The chat's copy, stored with the message. */
	media: PreparedMedia;
}

function pad(n: number): string {
	return String(n).padStart(2, '0');
}

/** A name that needs no quoting in a shell command: letters, digits, dots, dashes. */
function safeName(name: string): string {
	const ext = extname(name)
		.toLowerCase()
		.replace(/[^.a-z0-9]/g, '');
	const stem = basename(name, extname(name))
		.normalize('NFKD')
		.replace(/[^\w.-]+/g, '-')
		.replace(/^[-.]+|[-.]+$/g, '')
		.slice(0, 60);
	return `${stem || 'picture'}${ext}`;
}

/**
 * Saves the files, only if every one of them is a picture: otherwise nothing is kept and it
 * throws, saying which file. Returns them in the order given.
 */
export async function saveUploads(
	profileSlug: string,
	files: { name: string; data: Uint8Array }[]
): Promise<Attachment[]> {
	if (files.length > MAX_UPLOADS) throw new Error(`Attach at most ${MAX_UPLOADS} pictures.`);
	for (const file of files) {
		if (file.data.byteLength > MAX_UPLOAD_BYTES) {
			throw new Error(`${file.name} is larger than ${MAX_UPLOAD_BYTES / (1024 * 1024)} MB.`);
		}
		if (file.data.byteLength === 0) throw new Error(`${file.name} is empty.`);
	}
	const now = new Date();
	const day = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
	const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
	const dir = join(profileDir(profileSlug), 'uploads', day);
	mkdirSync(dir, { recursive: true });

	const saved: Attachment[] = [];
	try {
		for (const file of files) {
			const name = safeName(file.name);
			let path = join(dir, `${time}-${name}`);
			for (let n = 2; existsSync(path); n++) {
				path = join(dir, `${time}-${basename(name, extname(name))}-${n}${extname(name)}`);
			}
			writeFileSync(path, file.data, { flag: 'wx' });
			const media = await copyFile(path);
			saved.push({ path, media });
			if (media.status !== 'ok') throw new Error(`Couldn't save ${file.name}: ${media.error}.`);
			if (!media.mime?.startsWith('image/')) throw new Error(`${file.name} is not a picture.`);
		}
	} catch (err) {
		// The media copies no row points to are removed by the hourly prune.
		for (const { path } of saved) rmSync(path, { force: true });
		throw err;
	}
	return saved;
}
