import { createHash, randomUUID } from 'node:crypto';
import { constants, copyFileSync, existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { basename, extname, join } from 'node:path';
import type { Readable } from 'node:stream';
import type Anthropic from '@anthropic-ai/sdk';
import { and, eq, inArray, lt } from 'drizzle-orm';
import { countDocumentTokens, shortApiError } from './anthropic.ts';
import type { Conversation, MessageRow } from './conversations.ts';
import { getDb } from './db/index.ts';
import { upload } from './db/schema.ts';
import {
	MAX_CONVERSATION_IMAGE_BYTES,
	MAX_CONVERSATION_IMAGES,
	base64Length,
	imageUse,
	prepareImage,
	type ImageMediaType,
	type ImageUse,
	type ViewedImage
} from './images.ts';
import {
	blobPath,
	describeStored,
	newMediaId,
	store,
	storedType,
	type PreparedMedia
} from './media.ts';
import { profileDir } from './paths.ts';
import type { Provider } from './presets.ts';
import { providerFileId } from './provider-files.ts';

/*
 * Files people attach to a message. Each is saved in the profile's `attachments` folder, where
 * the agent can work with it, and shown in the chat through a media row. The model gets pictures
 * and PDFs themselves, through the provider's Files API, and every other file as its name and
 * path. What the model got is written into the message's `content` in the provider's format;
 * `message.attachments` keeps the provider-neutral record.
 */

export const MAX_ATTACHMENTS = 10;
/** Share of the model's context window that the PDFs of one conversation may fill, together. */
const DOCUMENT_SHARE = 0.25;
const DEFAULT_CONTEXT_WINDOW = 200_000;
/** Files attached in the composer but never sent are dropped after a day. */
const UPLOAD_TTL_MS = 24 * 60 * 60 * 1000;

export type UploadRow = typeof upload.$inferSelect;

export interface MessageAttachment {
	/** The media row that shows it in the chat. */
	mediaId: string;
	name: string;
	/** Where it was saved on this computer. */
	path: string;
	mime: string;
	bytes: number;
	/** What the model got: the picture, the document, or only the name and path. */
	sentAs: 'image' | 'document' | 'path';
	/** Documents: their size in tokens, counted against the conversation's budget. */
	tokens?: number;
	/** Why a picture or PDF went as its path only, in plain words. */
	note?: string;
}

/** Something wrong with what was attached, in words for the person sending it. */
export class AttachmentError extends Error {}

/** A name that's safe to save under: no folders, no control characters, not hidden. */
export function cleanFileName(name: string): string {
	const base = basename(name.replace(/\\/g, '/'))
		// eslint-disable-next-line no-control-regex
		.replace(/[\x00-\x1f\x7f]/g, '')
		.replace(/^\.+/, '')
		.trim();
	if (!base) return 'file';
	if (base.length <= 120) return base;
	const ext = extname(base).slice(0, 16);
	return base.slice(0, 120 - ext.length) + ext;
}

// --- uploads (attached, not sent yet) ---

/** Keeps a file someone attached in the composer until they send the message. */
export async function createUpload(input: {
	profileId: string;
	userId: string;
	name: string;
	body: Readable;
	signal?: AbortSignal;
}): Promise<UploadRow> {
	const name = cleanFileName(input.name);
	const stored = await store(input.body, input.signal);
	return getDb()
		.insert(upload)
		.values({
			id: randomUUID(),
			profileId: input.profileId,
			userId: input.userId,
			name,
			sha256: stored.sha256,
			mime: storedType(stored.sha256, name),
			bytes: stored.bytes,
			createdAt: new Date()
		})
		.returning()
		.get();
}

/** Removes a file from the composer. Only the person who attached it can. */
export function deleteUpload(profileId: string, userId: string, id: string): boolean {
	const removed = getDb()
		.delete(upload)
		.where(and(eq(upload.id, id), eq(upload.profileId, profileId), eq(upload.userId, userId)))
		.returning({ id: upload.id })
		.all();
	return removed.length > 0;
}

/** The sender's uploads with these ids, in that order. Throws if one is gone or someone else's. */
export function findUploads(profileId: string, userId: string, ids: string[]): UploadRow[] {
	if (!ids.length) return [];
	if (ids.length > MAX_ATTACHMENTS) {
		throw new AttachmentError(`At most ${MAX_ATTACHMENTS} files per message.`);
	}
	if (new Set(ids).size !== ids.length) throw new AttachmentError('A file is attached twice.');
	const rows = getDb()
		.select()
		.from(upload)
		.where(and(eq(upload.profileId, profileId), eq(upload.userId, userId), inArray(upload.id, ids)))
		.all();
	const byId = new Map(rows.map((row) => [row.id, row]));
	return ids.map((id) => {
		const row = byId.get(id);
		if (!row) throw new AttachmentError('An attached file is no longer there. Attach it again.');
		return row;
	});
}

export function pruneUploads(): void {
	getDb()
		.delete(upload)
		.where(lt(upload.createdAt, new Date(Date.now() - UPLOAD_TTL_MS)))
		.run();
}

// --- saving ---

export function attachmentsDir(profileSlug: string): string {
	return join(profileDir(profileSlug), 'attachments');
}

/** Whether the file at `path` still holds exactly this content. */
export function sameContent(path: string, sha256: string, bytes: number): boolean {
	try {
		if (statSync(path).size !== bytes) return false;
		return createHash('sha256').update(readFileSync(path)).digest('hex') === sha256;
	} catch {
		return false;
	}
}

/**
 * Copies the stored file into `dir` (the attachments folder, or a chat folder's) under its own
 * name, or `name (2).ext` when another file has that name. The same file sent again is saved once.
 */
export function saveAttachment(dir: string, name: string, sha256: string, bytes: number): string {
	mkdirSync(dir, { recursive: true });
	const ext = extname(name);
	const stem = name.slice(0, name.length - ext.length);
	for (let n = 1; ; n++) {
		const path = join(dir, n === 1 ? name : `${stem} (${n})${ext}`);
		if (existsSync(path)) {
			if (sameContent(path, sha256, bytes)) return path;
			continue;
		}
		try {
			copyFileSync(blobPath(sha256), path, constants.COPYFILE_EXCL);
			return path;
		} catch (err) {
			if ((err as NodeJS.ErrnoException).code !== 'EEXIST') throw err;
		}
	}
}

export function parseAttachments(json: string | null): MessageAttachment[] {
	if (!json) return [];
	try {
		const parsed = JSON.parse(json) as unknown;
		return Array.isArray(parsed) ? (parsed as MessageAttachment[]) : [];
	} catch {
		return [];
	}
}

// --- what the model gets (the provider's format) ---

type ImageBlock = Anthropic.ImageBlockParam;
type AttachmentBlock =
	Anthropic.TextBlockParam | Anthropic.ImageBlockParam | Anthropic.DocumentBlockParam;

/**
 * A picture as the provider's content block, counted in `used`: uploaded through its Files API,
 * or inline as base64 when that fails and the conversation still has room for it.
 */
export async function imageBlock(
	provider: Provider,
	data: Buffer,
	mediaType: ImageMediaType,
	name: string,
	used: ImageUse
): Promise<{ block: ImageBlock } | { problem: string }> {
	if (used.count >= MAX_CONVERSATION_IMAGES) {
		return {
			problem: `this conversation already holds ${MAX_CONVERSATION_IMAGES} pictures, as many as it can`
		};
	}
	let block: ImageBlock;
	try {
		const fileId = await providerFileId(provider, data, name, mediaType);
		block = { type: 'image', source: { type: 'file', file_id: fileId } };
	} catch (err) {
		const bytes = base64Length(data.length);
		if (used.bytes + bytes > MAX_CONVERSATION_IMAGE_BYTES) {
			return { problem: `it couldn't be uploaded (${shortApiError(err)})` };
		}
		console.error(`[btw] uploading ${name} failed, sending it inline: ${shortApiError(err)}`);
		block = {
			type: 'image',
			source: { type: 'base64', media_type: mediaType, data: data.toString('base64') }
		};
		used.bytes += bytes;
	}
	used.count++;
	return { block };
}

/** The images `btw view` left, for the command's tool_result, each after a line naming it. */
export async function viewedImageBlocks(
	provider: Provider,
	images: ViewedImage[],
	used: ImageUse
): Promise<(Anthropic.TextBlockParam | ImageBlock)[]> {
	const blocks: (Anthropic.TextBlockParam | ImageBlock)[] = [];
	for (const image of images) {
		const result =
			'problem' in image
				? { problem: image.problem }
				: await imageBlock(provider, image.data, image.mediaType, image.name, used);
		if ('problem' in result) {
			blocks.push({ type: 'text', text: `Not attached: ${image.name} (${result.problem}).` });
		} else {
			blocks.push({ type: 'text', text: `Image: ${image.name}` }, result.block);
		}
	}
	return blocks;
}

async function pdfBlock(
	conv: Conversation,
	path: string,
	name: string,
	room: number
): Promise<{ block: Anthropic.DocumentBlockParam; tokens: number } | { problem: string }> {
	let fileId: string;
	try {
		fileId = await providerFileId(conv.provider, readFileSync(path), name, 'application/pdf');
	} catch (err) {
		return { problem: `it couldn't be uploaded (${shortApiError(err)})` };
	}
	let tokens: number;
	try {
		tokens = await countDocumentTokens(conv.model, fileId);
	} catch (err) {
		return { problem: `the model can't read it (${shortApiError(err)})` };
	}
	if (tokens > room) {
		return {
			problem: `it's about ${tokens.toLocaleString('en-US')} tokens, more than this conversation has room for (${Math.max(0, room).toLocaleString('en-US')})`
		};
	}
	return {
		block: { type: 'document', source: { type: 'file', file_id: fileId }, title: name },
		tokens
	};
}

function attachmentLabel(senderName: string, attachment: MessageAttachment): string {
	const note = attachment.note ? `. It isn't shown here: ${attachment.note}` : '';
	return `[${senderName} attached ${attachment.name}, saved at ${attachment.path}${note}]`;
}

export interface PreparedMessage {
	/** The message's content in the provider's format. */
	content: AttachmentBlock[];
	attachments: MessageAttachment[];
	/** Media rows that show the attachments in the chat, with their ids. */
	media: (PreparedMedia & { id: string })[];
}

/**
 * A human message with attachments. Each attachment is saved to the attachments folder and gets
 * a line saying who attached it and where it is, followed by the picture or PDF itself when the
 * model can have it; the text comes last, as `Name: text`. `earlier`: the conversation's rows so
 * far, whose pictures and PDFs count against its limits.
 */
export async function prepareMessage(input: {
	conv: Conversation;
	profileSlug: string;
	senderName: string;
	text: string;
	uploads: UploadRow[];
	earlier: MessageRow[];
}): Promise<PreparedMessage> {
	const { conv, senderName } = input;
	if (conv.provider !== 'anthropic')
		throw new Error(`Attachments aren't supported for ${conv.provider}`);
	const dir = attachmentsDir(input.profileSlug);
	const images = imageUse(
		input.earlier.map((row) => ({ role: row.role, content: JSON.parse(row.content) }))
	);
	let documentTokens = input.earlier
		.flatMap((row) => parseAttachments(row.attachments))
		.reduce((sum, a) => sum + (a.sentAs === 'document' ? (a.tokens ?? 0) : 0), 0);
	const documentBudget = Math.floor(
		(conv.contextWindow ?? DEFAULT_CONTEXT_WINDOW) * DOCUMENT_SHARE
	);

	const prepared: PreparedMessage = { content: [], attachments: [], media: [] };
	for (const up of input.uploads) {
		const path = saveAttachment(dir, up.name, up.sha256, up.bytes);
		const media = {
			...(await describeStored(
				{ sha256: up.sha256, bytes: up.bytes, mime: up.mime },
				up.name,
				path
			)),
			id: newMediaId()
		};
		const attachment: MessageAttachment = {
			mediaId: media.id,
			name: up.name,
			path,
			mime: up.mime,
			bytes: up.bytes,
			sentAs: 'path'
		};
		let block: AttachmentBlock | null = null;
		if (up.mime.startsWith('image/')) {
			let result: { block: ImageBlock } | { problem: string };
			try {
				const image = await prepareImage(path);
				result = await imageBlock(conv.provider, image.data, image.info.mediaType, up.name, images);
			} catch (err) {
				result = {
					problem: `it couldn't be made into a picture the model can see (${(err as Error).message})`
				};
			}
			if ('problem' in result) attachment.note = result.problem;
			else {
				block = result.block;
				attachment.sentAs = 'image';
			}
		} else if (up.mime === 'application/pdf') {
			const result = await pdfBlock(conv, path, up.name, documentBudget - documentTokens);
			if ('problem' in result) attachment.note = result.problem;
			else {
				block = result.block;
				attachment.sentAs = 'document';
				attachment.tokens = result.tokens;
				documentTokens += result.tokens;
			}
		}
		prepared.content.push({ type: 'text', text: attachmentLabel(senderName, attachment) });
		if (block) prepared.content.push(block);
		prepared.attachments.push(attachment);
		prepared.media.push(media);
	}
	if (input.text) prepared.content.push({ type: 'text', text: `${senderName}: ${input.text}` });
	return prepared;
}
