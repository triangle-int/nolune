import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { and, eq, like, lt, or } from 'drizzle-orm';
import { anthropicFiles } from './anthropic.ts';
import { getDb } from './db/index.ts';
import { message, providerFile } from './db/schema.ts';
import type { Block, ImageBlock, Message, PdfBlock, Source, TextBlock } from './format.ts';
import { MAX_CONVERSATION_IMAGE_BYTES, base64Length } from './images.ts';
import { blobPath, referencedMedia } from './media.ts';
import { PROVIDER_LABELS, shortApiError, type Provider } from './models.ts';
import { openaiFiles } from './openai-chat.ts';
import { openrouterFiles } from './openrouter.ts';

/**
 * Pictures and PDFs kept on the provider's side (Anthropic's, OpenAI's or OpenRouter's Files API),
 * so a request refers to them by id instead of carrying their bytes, which it would resend with
 * every step for the rest of the conversation. Each provider brings its own store; the cache here
 * is shared.
 */
export interface FileStore {
	/** The account files live in. An id from one account means nothing in another. */
	account(): string;
	/** Returns the provider's id for the file. */
	upload(data: Buffer, name: string, mime: string): Promise<string>;
	/** Whether the file is still there. */
	exists(fileId: string): Promise<boolean>;
	remove(fileId: string): Promise<void>;
}

/** Chats on the Claude plan have none: Claude Code gets pictures inline and PDFs as paths. */
const stores = {
	anthropic: anthropicFiles,
	openai: openaiFiles,
	openrouter: openrouterFiles
} satisfies Partial<Record<Provider, FileStore>>;

export function hasFileStore(provider: Provider): provider is keyof typeof stores {
	return Object.hasOwn(stores, provider);
}

/** Uploads under way, so the same content sent twice at once is uploaded once. */
const inFlight = new Map<string, Promise<string>>();

/** A message is about to be saved with an id: files in use this recently are never deleted. */
const IN_USE_MS = 60 * 60 * 1000;

/**
 * The provider's id for this content, uploading it the first time it's needed. A cached id is
 * checked first: a message referring to a file that's gone would fail every later request.
 */
export async function providerFileId(
	provider: Provider,
	data: Buffer,
	name: string,
	mime: string
): Promise<string> {
	if (!hasFileStore(provider)) throw new Error(`${provider} has no Files API`);
	const store = stores[provider];
	const account = store.account();
	const sha256 = createHash('sha256').update(data).digest('hex');
	const where = and(
		eq(providerFile.provider, provider),
		eq(providerFile.account, account),
		eq(providerFile.sha256, sha256)
	);
	// Marked as used before the check, so the prune leaves it alone from here on.
	const known = getDb()
		.update(providerFile)
		.set({ usedAt: new Date() })
		.where(where)
		.returning({ fileId: providerFile.fileId })
		.get();
	if (known) {
		if (await store.exists(known.fileId)) return known.fileId;
		getDb().delete(providerFile).where(where).run();
	}

	const key = `${provider}:${account}:${sha256}`;
	let pending = inFlight.get(key);
	if (!pending) {
		pending = store
			.upload(data, name, mime)
			.then((fileId) => {
				getDb()
					.insert(providerFile)
					.values({ provider, account, sha256, fileId })
					.onConflictDoNothing()
					.run();
				return fileId;
			})
			.finally(() => inFlight.delete(key));
		inFlight.set(key, pending);
	}
	return pending;
}

// --- pictures and PDFs kept by reference ---

type FileBlock = ImageBlock | PdfBlock;
type MediaSource = Extract<Source, { type: 'media' }>;

/** Uploads at once, for the first request after a conversation switched to a provider. */
const UPLOADS_AT_ONCE = 4;

/**
 * The provider's copy of a file btw keeps, uploading it the first time. A known copy is taken as
 * it is, unlike in providerFileId: every request carries every picture, and checking each one
 * would cost a request apiece.
 */
async function mediaFileId(
	provider: keyof typeof stores,
	source: MediaSource,
	name: string
): Promise<string> {
	const known = getDb()
		.update(providerFile)
		.set({ usedAt: new Date() })
		.where(
			and(
				eq(providerFile.provider, provider),
				eq(providerFile.account, stores[provider].account()),
				eq(providerFile.sha256, source.sha256)
			)
		)
		.returning({ fileId: providerFile.fileId })
		.get();
	if (known) return known.fileId;
	return providerFileId(provider, mediaBytes(source, name), name, source.mime);
}

function mediaBytes(source: MediaSource, name: string): Buffer {
	const path = blobPath(source.sha256);
	if (!existsSync(path)) throw new Error(`${name} is no longer on this computer.`);
	return readFileSync(path);
}

/** Its name, for the Files API: a PDF's own, or one for a picture from its type. */
function uploadName(block: FileBlock): string {
	if (block.type === 'pdf') return block.name || 'document.pdf';
	return `picture.${block.source.type === 'media' ? block.source.mime.split('/')[1] : 'png'}`;
}

function fileBlocks(messages: Message[]): FileBlock[] {
	const found: FileBlock[] = [];
	const add = (b: Block) => {
		if ((b.type === 'image' || b.type === 'pdf') && b.source.type === 'media') found.push(b);
	};
	for (const m of messages) {
		for (const b of m.blocks) {
			add(b);
			if (b.type === 'tool_result' && Array.isArray(b.content)) b.content.forEach(add);
		}
	}
	return found;
}

/** What a model reads instead of a picture or PDF past what the conversation takes inline. */
function overLimitNote(block: FileBlock): TextBlock {
	const what = block.type === 'image' ? 'Picture' : 'PDF';
	return {
		type: 'text',
		text: `[${what} not shown: this chat already holds as many pictures and PDFs as this model takes. The line before this says where its file is${block.type === 'image' ? '; `btw view` shows it again' : ''}.]`
	};
}

/**
 * The messages with each picture and PDF btw keeps by reference (`media` sources) as `provider`
 * gets it: its Files API's copy, uploaded the first time a request on it needs one, or inline
 * where it has none (the Claude plan), oldest first up to the conversation's inline limit, the
 * rest as a note. The same rows give the same request every time: a copy is reused for as long as
 * the cache has it (pruneProviderFiles keeps it while a message refers to it). Other messages are
 * returned as they are. Throws when an upload fails: that call fails, and Continue tries again.
 */
export async function resolveFiles(messages: Message[], provider: Provider): Promise<Message[]> {
	const blocks = fileBlocks(messages);
	if (!blocks.length) return messages;
	const ids = new Map<string, string>();
	if (hasFileStore(provider)) {
		const pending = [...new Map(blocks.map((b) => [(b.source as MediaSource).sha256, b])).values()];
		const upload = async () => {
			for (let b = pending.shift(); b; b = pending.shift()) {
				const source = b.source as MediaSource;
				try {
					ids.set(source.sha256, await mediaFileId(provider, source, uploadName(b)));
				} catch (err) {
					throw new Error(
						`Couldn't give ${PROVIDER_LABELS[provider]} ${b.type === 'pdf' ? b.name : 'a picture'} from this chat: ${shortApiError(err)}`,
						{ cause: err }
					);
				}
			}
		};
		await Promise.all(Array.from({ length: UPLOADS_AT_ONCE }, upload));
	}

	let inlineBytes = 0;
	const resolve = (b: Block): Block => {
		if (b.type !== 'image' && b.type !== 'pdf') return b;
		const { source } = b;
		if (source.type === 'inline') inlineBytes += source.data.length;
		if (source.type !== 'media') return b;
		const fileId = ids.get(source.sha256);
		if (fileId && hasFileStore(provider)) {
			return { ...b, source: { type: 'uploaded', provider, fileId } };
		}
		const bytes = base64Length(source.bytes);
		if (inlineBytes + bytes > MAX_CONVERSATION_IMAGE_BYTES) return overLimitNote(b);
		inlineBytes += bytes;
		const data = mediaBytes(source, b.type === 'pdf' ? b.name : 'A picture').toString('base64');
		return { ...b, source: { type: 'inline', mime: source.mime, data } };
	};
	return messages.map((m) => {
		let changed = false;
		const blocks = m.blocks.map((b) => {
			let next: Block;
			if (b.type === 'tool_result' && Array.isArray(b.content)) {
				const content = b.content.map((c) => resolve(c) as typeof c);
				next = content.every((c, i) => c === (b.content as Block[])[i]) ? b : { ...b, content };
			} else next = resolve(b);
			if (next !== b) changed = true;
			return next;
		});
		return changed ? { ...m, blocks } : m;
	});
}

// --- pruning ---

function collectFileIds(value: unknown, ids: Set<string>): void {
	if (Array.isArray(value)) {
		for (const item of value) collectFileIds(item, ids);
	} else if (value && typeof value === 'object') {
		const record = value as Record<string, unknown>;
		// btw's format, and Anthropic's (rows from before it)
		if (record.type === 'uploaded' && typeof record.fileId === 'string') ids.add(record.fileId);
		if (record.type === 'file' && typeof record.file_id === 'string') ids.add(record.file_id);
		for (const child of Object.values(record)) collectFileIds(child, ids);
	}
}

/** Every file id a stored message refers to, in btw's format or a provider's. */
function referencedFileIds(): Set<string> {
	const ids = new Set<string>();
	const rows = getDb()
		.select({ content: message.content })
		.from(message)
		.where(or(like(message.content, '%"fileId"%'), like(message.content, '%"file_id"%')))
		.all();
	for (const row of rows) collectFileIds(JSON.parse(row.content), ids);
	return ids;
}

/**
 * Deletes the provider's copies that no message refers to any more (their conversations were
 * deleted or expired), by id or by what it's a copy of. Files in another account than the current
 * key's are left alone: they can't be reached, and the key may change back.
 */
export async function pruneProviderFiles(): Promise<void> {
	const rows = getDb()
		.select()
		.from(providerFile)
		.where(lt(providerFile.usedAt, new Date(Date.now() - IN_USE_MS)))
		.all();
	if (!rows.length) return;
	const used = referencedFileIds();
	// Copies of what messages keep by reference, which later requests send again.
	const media = referencedMedia();
	for (const row of rows) {
		if (used.has(row.fileId) || media.has(row.sha256)) continue;
		const store = stores[row.provider];
		const where = and(
			eq(providerFile.provider, row.provider),
			eq(providerFile.account, row.account),
			eq(providerFile.sha256, row.sha256)
		);
		// A message may have picked it up again since the list was read.
		const current = getDb()
			.select({ usedAt: providerFile.usedAt })
			.from(providerFile)
			.where(where)
			.get();
		if (!current || current.usedAt.getTime() >= Date.now() - IN_USE_MS) continue;
		try {
			if (store.account() !== row.account) continue;
			await store.remove(row.fileId);
		} catch (err) {
			console.error(`[btw] couldn't delete ${row.provider} file ${row.fileId}:`, err);
			continue;
		}
		getDb().delete(providerFile).where(where).run();
	}
}
