import { execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import {
	closeSync,
	createReadStream,
	createWriteStream,
	existsSync,
	linkSync,
	mkdirSync,
	openSync,
	readSync,
	readdirSync,
	realpathSync,
	renameSync,
	rmSync,
	statSync,
	utimesSync
} from 'node:fs';
import { homedir } from 'node:os';
import { basename, extname, isAbsolute, join, resolve } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import type { ReadableStream as WebReadableStream } from 'node:stream/web';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { and, eq } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { media } from './db/schema.ts';
import { isRemoteHref, mediaRefs } from './media-refs.ts';
import { paths } from './paths.ts';

/**
 * Pictures and files in the agent's replies. When a reply is saved, every picture
 * (`![alt](src)`) and file link (`[label](src)`) it contains is copied byte for byte into
 * `~/.btw-agent/media/<sha256>`, so the chat keeps showing it after the original moves or
 * disappears. The web UI only ever loads these copies, through ids checked against the
 * conversation.
 */

export type MediaRow = typeof media.$inferSelect;
export type MediaStatus = MediaRow['status'];

/** A copy made for a reply that isn't saved yet: the row without its ids. */
export type PreparedMedia = Omit<MediaRow, 'id' | 'conversationId' | 'messageId' | 'createdAt'>;

/** What the chat needs to show one picture or file, keyed by its link target in the reply. */
export type DisplayMedia =
	| {
			status: 'ok';
			id: string;
			name: string;
			mime: string;
			bytes: number;
			width: number | null;
			height: number | null;
			/** Shown as a picture (the original, or a JPEG copy of it) rather than as a download. */
			viewable: boolean;
	  }
	| { status: Exclude<MediaStatus, 'ok'>; name: string; error: string };

export const MAX_MEDIA_BYTES = 100 * 1024 * 1024;
const MAX_PER_REPLY = 30;
const FETCH_TIMEOUT_MS = 30_000;
const CONVERT_TIMEOUT_MS = 60_000;
/** Enough to find the size of a JPEG behind a large EXIF block. */
const HEAD_BYTES = 1024 * 1024;
/** Unreferenced files younger than this are kept: a copy may be about to get its row. */
const ORPHAN_GRACE_MS = 60 * 60 * 1000;

/** Picture types every current browser shows in an <img>. */
const VIEWABLE = new Set([
	'image/png',
	'image/jpeg',
	'image/gif',
	'image/webp',
	'image/avif',
	'image/svg+xml',
	'image/bmp',
	'image/x-icon'
]);
/** Pictures browsers can't show, displayed through a JPEG copy made with macOS's `sips`. */
const CONVERTIBLE = new Set(['image/heic', 'image/heif', 'image/tiff']);

const IMAGE_EXTENSIONS: Record<string, string> = {
	'image/png': '.png',
	'image/jpeg': '.jpg',
	'image/gif': '.gif',
	'image/webp': '.webp',
	'image/avif': '.avif',
	'image/svg+xml': '.svg',
	'image/bmp': '.bmp',
	'image/x-icon': '.ico',
	'image/heic': '.heic',
	'image/heif': '.heif',
	'image/tiff': '.tiff'
};

/** For files whose content doesn't say what they are. */
const EXTENSION_TYPES: Record<string, string> = {
	'.txt': 'text/plain',
	'.md': 'text/markdown',
	'.csv': 'text/csv',
	'.html': 'text/html',
	'.htm': 'text/html',
	'.json': 'application/json',
	'.xml': 'application/xml',
	'.ics': 'text/calendar',
	'.vcf': 'text/vcard',
	'.rtf': 'application/rtf',
	'.zip': 'application/zip',
	'.epub': 'application/epub+zip',
	'.doc': 'application/msword',
	'.docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
	'.xls': 'application/vnd.ms-excel',
	'.xlsx': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
	'.ppt': 'application/vnd.ms-powerpoint',
	'.pptx': 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
	'.mp3': 'audio/mpeg',
	'.m4a': 'audio/mp4',
	'.wav': 'audio/wav',
	'.mp4': 'video/mp4',
	'.mov': 'video/quicktime'
};

export function blobPath(sha256: string): string {
	return join(paths.media, sha256);
}

export function isViewable(row: Pick<MediaRow, 'mime' | 'previewSha256'>): boolean {
	return !!row.previewSha256 || (!!row.mime && VIEWABLE.has(row.mime));
}

function formatMegabytes(bytes: number): string {
	return `${Math.round(bytes / (1024 * 1024))} MB`;
}

// --- copying ---

class TooLargeError extends Error {}

/** Streams into the store under the content's SHA-256. The same content is stored once. */
async function store(
	source: Readable,
	signal?: AbortSignal
): Promise<{ sha256: string; bytes: number }> {
	mkdirSync(paths.media, { recursive: true });
	const tmp = join(paths.media, `tmp-${randomUUID()}`);
	const hash = createHash('sha256');
	let bytes = 0;
	try {
		await pipeline(
			source,
			async function* (chunks: AsyncIterable<Uint8Array>) {
				for await (const chunk of chunks) {
					bytes += chunk.length;
					if (bytes > MAX_MEDIA_BYTES) throw new TooLargeError();
					hash.update(chunk);
					yield chunk;
				}
			},
			createWriteStream(tmp),
			{ signal }
		);
	} catch (err) {
		rmSync(tmp, { force: true });
		throw err;
	}
	const sha256 = hash.digest('hex');
	const dest = blobPath(sha256);
	if (existsSync(dest)) {
		rmSync(tmp, { force: true });
		// Fresh again, so the orphan sweep leaves it alone until the row is saved.
		const now = new Date();
		utimesSync(dest, now, now);
	} else {
		renameSync(tmp, dest);
	}
	return { sha256, bytes };
}

function resolveLocal(href: string, baseDir: string): string {
	if (/^file:/i.test(href)) {
		try {
			return fileURLToPath(href);
		} catch {
			return href;
		}
	}
	if (href === '~') return homedir();
	if (href.startsWith('~/')) return join(homedir(), href.slice(2));
	return isAbsolute(href) ? href : resolve(baseDir, href);
}

/** The file a local link points to, also trying it URL-decoded (`My%20Photos`). */
function findLocal(href: string, baseDir: string): string | null {
	const candidates = [resolveLocal(href, baseDir)];
	try {
		const decoded = decodeURIComponent(href);
		if (decoded !== href) candidates.push(resolveLocal(decoded, baseDir));
	} catch {
		// not URL-encoded
	}
	return candidates.find((file) => existsSync(file)) ?? null;
}

/** btw's own secrets never leave through a chat, whatever a reply links to. */
function isPrivate(realFile: string): boolean {
	const db = paths.db;
	return [paths.config, db, `${db}-wal`, `${db}-shm`, `${db}-journal`].some((file) => {
		try {
			return realpathSync(file) === realFile;
		} catch {
			return file === realFile;
		}
	});
}

function nameFromHref(href: string): string {
	if (isRemoteHref(href)) {
		try {
			return decodeURIComponent(basename(new URL(href).pathname));
		} catch {
			return '';
		}
	}
	return basename(href.replace(/[\\/]+$/, ''));
}

function describeError(err: unknown): string {
	const e = err as { name?: string; code?: string; cause?: { code?: string; message?: string } };
	if (e?.name === 'TimeoutError') return 'Downloading it took too long';
	if (e?.name === 'AbortError') return 'Stopped before it was copied';
	if (e?.code === 'EACCES' || e?.code === 'EPERM') {
		return 'btw is not allowed to read this file (on a Mac, check Full Disk Access)';
	}
	if (e?.cause) return `Couldn't download it (${e.cause.code ?? e.cause.message})`;
	return err instanceof Error ? err.message : String(err);
}

const execFileAsync = promisify(execFile);

/** A full-size JPEG copy of a HEIC or TIFF picture, on macOS. Null where `sips` isn't there. */
async function convertToJpeg(sha256: string, mime: string): Promise<string | null> {
	if (process.platform !== 'darwin') return null;
	const id = randomUUID();
	// sips reads the format from the file name, so it gets a link with the right extension.
	const input = join(paths.media, `tmp-${id}${IMAGE_EXTENSIONS[mime] ?? ''}`);
	const output = join(paths.media, `tmp-${id}.jpg`);
	try {
		linkSync(blobPath(sha256), input);
		await execFileAsync('/usr/bin/sips', ['-s', 'format', 'jpeg', input, '--out', output], {
			timeout: CONVERT_TIMEOUT_MS
		});
		return (await store(createReadStream(output))).sha256;
	} catch (err) {
		console.error(`[btw] couldn't convert a ${mime} picture to JPEG:`, err);
		return null;
	} finally {
		rmSync(input, { force: true });
		rmSync(output, { force: true });
	}
}

async function copyOne(href: string, baseDir: string, signal: AbortSignal): Promise<PreparedMedia> {
	let name = nameFromHref(href) || 'file';
	const problem = (status: Exclude<MediaStatus, 'ok'>, error: string): PreparedMedia => ({
		src: href,
		status,
		error,
		name,
		sha256: null,
		mime: null,
		bytes: null,
		width: null,
		height: null,
		previewSha256: null
	});
	const tooLarge = () => problem('too_large', `Larger than ${formatMegabytes(MAX_MEDIA_BYTES)}`);

	try {
		let stored: { sha256: string; bytes: number };
		const remote = isRemoteHref(href);
		if (remote) {
			const res = await fetch(href, {
				signal: AbortSignal.any([signal, AbortSignal.timeout(FETCH_TIMEOUT_MS)]),
				headers: { accept: 'image/*,*/*;q=0.5', 'user-agent': 'btw-agent' }
			});
			if (!res.ok || !res.body) {
				await res.body?.cancel();
				return problem('failed', `Couldn't download it (the server answered ${res.status})`);
			}
			if (Number(res.headers.get('content-length')) > MAX_MEDIA_BYTES) {
				await res.body.cancel();
				return tooLarge();
			}
			stored = await store(Readable.fromWeb(res.body as WebReadableStream), signal);
		} else {
			const file = findLocal(href, baseDir);
			if (!file) return problem('missing', 'File not found');
			name = basename(file);
			const real = realpathSync(file);
			const info = statSync(real);
			if (info.isDirectory()) return problem('unsupported', 'This is a folder, not a file');
			if (!info.isFile()) return problem('unsupported', 'This is not a regular file');
			if (isPrivate(real)) {
				return problem('blocked', "btw's own settings and database are never shared");
			}
			if (info.size > MAX_MEDIA_BYTES) return tooLarge();
			stored = await store(createReadStream(real), signal);
		}

		const head = readHead(blobPath(stored.sha256));
		const mime = sniffType(head, name);
		// A web link in a picture that turns out to be a page gets no row pointing at the copy;
		// the orphan sweep removes it.
		if (remote && !mime.startsWith('image/')) {
			return problem('unsupported', 'The link is not a picture');
		}
		if (remote && !extname(name) && IMAGE_EXTENSIONS[mime]) {
			name = `${name === 'file' ? 'picture' : name}${IMAGE_EXTENSIONS[mime]}`;
		}
		let previewSha256: string | null = null;
		let size = imageSize(head, mime);
		if (CONVERTIBLE.has(mime)) {
			previewSha256 = await convertToJpeg(stored.sha256, mime);
			if (previewSha256) size = imageSize(readHead(blobPath(previewSha256)), 'image/jpeg');
		}
		return {
			src: href,
			status: 'ok',
			error: null,
			name,
			sha256: stored.sha256,
			mime,
			bytes: stored.bytes,
			width: size?.width ?? null,
			height: size?.height ?? null,
			previewSha256
		};
	} catch (err) {
		if (err instanceof TooLargeError) return tooLarge();
		return problem('failed', describeError(err));
	}
}

/**
 * Copies the pictures and files that a reply's text links to. Runs before the reply is saved,
 * so the reply and its copies appear in the transcript together. Never throws: whatever can't
 * be copied gets a row that says why, and the chat shows that instead.
 */
export async function copyReplyMedia(
	texts: string[],
	baseDir: string,
	signal: AbortSignal
): Promise<PreparedMedia[]> {
	let refs: string[];
	try {
		refs = [...new Set(texts.flatMap((text) => mediaRefs(text)))];
	} catch (err) {
		console.error('[btw] reading the links in a reply failed:', err);
		return [];
	}
	return Promise.all(
		refs.map((href, i) =>
			i < MAX_PER_REPLY
				? copyOne(href, baseDir, signal)
				: Promise.resolve<PreparedMedia>({
						src: href,
						status: 'failed',
						error: `Only the first ${MAX_PER_REPLY} pictures and files of a reply are kept`,
						name: nameFromHref(href) || 'file',
						sha256: null,
						mime: null,
						bytes: null,
						width: null,
						height: null,
						previewSha256: null
					})
		)
	);
}

// --- reading files ---

function readHead(file: string, length = HEAD_BYTES): Buffer {
	const fd = openSync(file, 'r');
	try {
		const buf = Buffer.alloc(length);
		return buf.subarray(0, readSync(fd, buf, 0, length, 0));
	} finally {
		closeSync(fd);
	}
}

const SVG = /^\s*(?:<\?xml[^>]*>\s*)?(?:<!--[\s\S]*?-->\s*|<!DOCTYPE[^>]*>\s*)*<svg[\s>]/i;

/** The type from the content, falling back to the extension. Never trusts an extension for pictures. */
function sniffType(head: Buffer, name: string): string {
	const ascii = (start: number, end: number) => head.toString('latin1', start, end);
	if (ascii(0, 8) === '\x89PNG\r\n\x1a\n') return 'image/png';
	if (head[0] === 0xff && head[1] === 0xd8 && head[2] === 0xff) return 'image/jpeg';
	if (ascii(0, 6) === 'GIF87a' || ascii(0, 6) === 'GIF89a') return 'image/gif';
	if (ascii(0, 4) === 'RIFF' && ascii(8, 12) === 'WEBP') return 'image/webp';
	if (ascii(4, 8) === 'ftyp' && head.length >= 16) {
		// ISO media: the brands tell pictures (AVIF, HEIC) from videos (MP4, MOV).
		const end = Math.min(head.length, Math.max(16, head.readUInt32BE(0)), 128);
		const brands = [ascii(8, 12)];
		for (let i = 16; i + 4 <= end; i += 4) brands.push(ascii(i, i + 4));
		if (brands.some((b) => b === 'avif' || b === 'avis')) return 'image/avif';
		if (brands.some((b) => ['heic', 'heix', 'hevc', 'hevx', 'heim', 'heis'].includes(b))) {
			return 'image/heic';
		}
		if (brands.some((b) => b === 'mif1' || b === 'msf1')) return 'image/heif';
	}
	// TIFF, and camera RAW files built on it (DNG, CR2, NEF), which sips converts too.
	if (ascii(0, 4) === 'II*\0' || ascii(0, 4) === 'MM\0*') return 'image/tiff';
	if (ascii(0, 2) === 'BM' && head.length > 26 && [12, 40, 52, 56, 108, 124].includes(head[14])) {
		return 'image/bmp';
	}
	if (head.length > 6 && head.readUInt32BE(0) === 0x100 && head.readUInt16LE(4) > 0) {
		return 'image/x-icon';
	}
	if (ascii(0, 5) === '%PDF-') return 'application/pdf';
	if (SVG.test(head.toString('utf8', 0, 4096).replace(/^\uFEFF/, ''))) return 'image/svg+xml';
	return EXTENSION_TYPES[extname(name).toLowerCase()] ?? 'application/octet-stream';
}

interface Size {
	width: number;
	height: number;
}

/** Pixel size from the header, as displayed (a rotated JPEG's sides swapped). */
function imageSize(head: Buffer, mime: string): Size | null {
	let size: Size | null = null;
	try {
		if (mime === 'image/png')
			size = { width: head.readUInt32BE(16), height: head.readUInt32BE(20) };
		else if (mime === 'image/gif') {
			size = { width: head.readUInt16LE(6), height: head.readUInt16LE(8) };
		} else if (mime === 'image/bmp') {
			size = { width: head.readInt32LE(18), height: Math.abs(head.readInt32LE(22)) };
		} else if (mime === 'image/webp') size = webpSize(head);
		else if (mime === 'image/jpeg') size = jpegSize(head);
	} catch {
		// truncated or unusual header
	}
	return size && size.width > 0 && size.height > 0 ? size : null;
}

function webpSize(buf: Buffer): Size | null {
	const chunk = buf.toString('latin1', 12, 16);
	if (chunk === 'VP8 ') {
		return { width: buf.readUInt16LE(26) & 0x3fff, height: buf.readUInt16LE(28) & 0x3fff };
	}
	if (chunk === 'VP8L') {
		const bits = buf.readUInt32LE(21);
		return { width: (bits & 0x3fff) + 1, height: ((bits >> 14) & 0x3fff) + 1 };
	}
	if (chunk === 'VP8X')
		return { width: buf.readUIntLE(24, 3) + 1, height: buf.readUIntLE(27, 3) + 1 };
	return null;
}

const JPEG_FRAME_MARKERS = new Set([
	0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf
]);

function jpegSize(buf: Buffer): Size | null {
	let orientation = 1;
	let i = 2;
	while (i + 9 < buf.length) {
		if (buf[i] !== 0xff) return null;
		const marker = buf[i + 1];
		if (marker === 0xff) {
			i++;
			continue;
		}
		if (marker === 0xd8 || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
			i += 2;
			continue;
		}
		if (marker === 0xda) return null; // image data starts: no frame header found
		const length = buf.readUInt16BE(i + 2);
		if (marker === 0xe1 && buf.toString('latin1', i + 4, i + 10) === 'Exif\0\0') {
			orientation = exifOrientation(buf.subarray(i + 10, i + 2 + length)) ?? orientation;
		}
		if (JPEG_FRAME_MARKERS.has(marker)) {
			const height = buf.readUInt16BE(i + 5);
			const width = buf.readUInt16BE(i + 7);
			// Orientations 5-8 turn the picture by 90°, which browsers apply when showing it.
			return orientation >= 5 && orientation <= 8
				? { width: height, height: width }
				: { width, height };
		}
		i += 2 + length;
	}
	return null;
}

function exifOrientation(tiff: Buffer): number | null {
	try {
		const little = tiff.toString('latin1', 0, 2) === 'II';
		const u16 = (at: number) => (little ? tiff.readUInt16LE(at) : tiff.readUInt16BE(at));
		const ifd = little ? tiff.readUInt32LE(4) : tiff.readUInt32BE(4);
		const count = u16(ifd);
		for (let k = 0; k < count; k++) {
			const entry = ifd + 2 + k * 12;
			if (u16(entry) === 0x0112) return u16(entry + 8);
		}
	} catch {
		// malformed EXIF
	}
	return null;
}

// --- database ---

export function newMediaId(): string {
	return randomUUID();
}

/** Every copy in the conversation, grouped by the reply it belongs to. */
export function mediaByMessage(conversationId: string): Map<number, MediaRow[]> {
	const byMessage = new Map<number, MediaRow[]>();
	const rows = getDb().select().from(media).where(eq(media.conversationId, conversationId)).all();
	for (const row of rows) {
		const list = byMessage.get(row.messageId);
		if (list) list.push(row);
		else byMessage.set(row.messageId, [row]);
	}
	return byMessage;
}

export function listMedia(messageId: number): MediaRow[] {
	return getDb().select().from(media).where(eq(media.messageId, messageId)).all();
}

export function getMedia(conversationId: string, id: string): MediaRow | undefined {
	return getDb()
		.select()
		.from(media)
		.where(and(eq(media.id, id), eq(media.conversationId, conversationId)))
		.get();
}

export function toDisplayMedia(rows: MediaRow[]): Record<string, DisplayMedia> {
	const out: Record<string, DisplayMedia> = {};
	for (const row of rows) {
		out[row.src] =
			row.status === 'ok' && row.mime && row.bytes !== null
				? {
						status: 'ok',
						id: row.id,
						name: row.name,
						mime: row.mime,
						bytes: row.bytes,
						width: row.width,
						height: row.height,
						viewable: isViewable(row)
					}
				: {
						status: row.status === 'ok' ? 'failed' : row.status,
						name: row.name,
						error: row.error ?? 'Not available'
					};
	}
	return out;
}

/**
 * The file to send: for viewing, the picture itself or its JPEG copy; for downloading, the
 * original. `inline` is false for anything that isn't a picture a browser shows.
 */
export function mediaFile(
	row: MediaRow,
	purpose: 'view' | 'download'
): { path: string; mime: string; name: string; inline: boolean } | null {
	if (row.status !== 'ok' || !row.sha256 || !row.mime) return null;
	if (purpose === 'view' && row.previewSha256) {
		const name = `${basename(row.name, extname(row.name))}.jpg`;
		return { path: blobPath(row.previewSha256), mime: 'image/jpeg', name, inline: true };
	}
	return {
		path: blobPath(row.sha256),
		mime: row.mime,
		name: row.name,
		inline: purpose === 'view' && VIEWABLE.has(row.mime)
	};
}

/**
 * Deletes stored files no row points to any more (their conversations were deleted). Files
 * younger than an hour are kept, since a reply being saved may be about to reference them.
 */
export function pruneMedia(): void {
	if (!existsSync(paths.media)) return;
	const used = new Set<string>();
	const rows = getDb()
		.select({ sha256: media.sha256, previewSha256: media.previewSha256 })
		.from(media)
		.all();
	for (const row of rows) {
		if (row.sha256) used.add(row.sha256);
		if (row.previewSha256) used.add(row.previewSha256);
	}
	const cutoff = Date.now() - ORPHAN_GRACE_MS;
	for (const name of readdirSync(paths.media)) {
		if (used.has(name)) continue;
		const file = join(paths.media, name);
		try {
			if (statSync(file).mtimeMs < cutoff) rmSync(file, { force: true });
		} catch {
			// already gone
		}
	}
}
