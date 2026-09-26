import { createHash } from 'node:crypto';
import { and, eq, like, lt } from 'drizzle-orm';
import { anthropicFiles } from './anthropic.ts';
import { getDb } from './db/index.ts';
import { message, providerFile } from './db/schema.ts';
import type { Provider } from './presets.ts';

/**
 * Pictures and PDFs kept on the provider's side (Anthropic's Files API), so a request refers to
 * them by id instead of carrying their bytes, which it would resend with every step for the
 * rest of the conversation. Each provider brings its own store; the cache here is shared.
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

const stores: Record<Provider, FileStore> = { anthropic: anthropicFiles };

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

function collectFileIds(value: unknown, ids: Set<string>): void {
	if (Array.isArray(value)) {
		for (const item of value) collectFileIds(item, ids);
	} else if (value && typeof value === 'object') {
		const record = value as Record<string, unknown>;
		if (record.type === 'file' && typeof record.file_id === 'string') ids.add(record.file_id);
		for (const child of Object.values(record)) collectFileIds(child, ids);
	}
}

/** Every file id a stored message refers to, in any provider's format. */
function referencedFileIds(): Set<string> {
	const ids = new Set<string>();
	const rows = getDb()
		.select({ content: message.content })
		.from(message)
		.where(like(message.content, '%"file_id"%'))
		.all();
	for (const row of rows) collectFileIds(JSON.parse(row.content), ids);
	return ids;
}

/**
 * Deletes the provider's copies that no message refers to any more (their conversations were
 * deleted or expired). Files in another account than the current key's are left alone: they
 * can't be reached, and the key may change back.
 */
export async function pruneProviderFiles(): Promise<void> {
	const rows = getDb()
		.select()
		.from(providerFile)
		.where(lt(providerFile.usedAt, new Date(Date.now() - IN_USE_MS)))
		.all();
	if (!rows.length) return;
	const used = referencedFileIds();
	for (const row of rows) {
		if (used.has(row.fileId)) continue;
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
