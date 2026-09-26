import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import {
	appendFileSync,
	existsSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	statSync,
	writeFileSync
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, extname, join } from 'node:path';
import type Anthropic from '@anthropic-ai/sdk';

/*
 * `btw view`: how the agent looks at images without a second tool. The gateway gives every
 * run_command call its own folder (BTW_VIEW_DIR); `btw view` prepares images and leaves them
 * there, and after the command exits the gateway attaches them to that call's tool_result.
 */

/**
 * Longest edge of an image sent to the model. Requests with more than 20 images (the history
 * included) reject anything larger, so bigger images would break a conversation later on.
 */
export const MAX_EDGE = 2000;
/** Larger files are re-encoded, since every image is resent with each step. */
const REENCODE_ABOVE_BYTES = 1_500_000;
/** Largest image after preparing it. The API allows 10 MB of base64 per image. */
const MAX_IMAGE_BYTES = 5_000_000;
const JPEG_QUALITY = '85';

export const MAX_IMAGES_PER_COMMAND = 10;
/**
 * Per conversation. Every request carries the whole history, and a request is capped at 32 MB
 * and, on 200k-context models, 100 images. Counted in base64 characters.
 */
export const MAX_CONVERSATION_IMAGES = 100;
export const MAX_CONVERSATION_IMAGE_BYTES = 20_000_000;

type MediaType = Anthropic.Base64ImageSource['media_type'];

export interface ImageInfo {
	mediaType: MediaType;
	width: number;
	height: number;
	/** EXIF orientation of a JPEG. 1 is upright; the model ignores it and sees the raw pixels. */
	orientation: number;
}

/** Type and size from the file's header, for the formats the API accepts. Null for anything else. */
export function inspectImage(buf: Buffer): ImageInfo | null {
	const info = readHeader(buf);
	return info && info.width > 0 && info.height > 0 ? info : null;
}

function readHeader(buf: Buffer): ImageInfo | null {
	try {
		if (buf.readUInt32BE(0) === 0x89504e47 && buf.toString('latin1', 12, 16) === 'IHDR') {
			const [width, height] = [buf.readUInt32BE(16), buf.readUInt32BE(20)];
			return { mediaType: 'image/png', width, height, orientation: 1 };
		}
		const head = buf.toString('latin1', 0, 6);
		if (head === 'GIF87a' || head === 'GIF89a') {
			const [width, height] = [buf.readUInt16LE(6), buf.readUInt16LE(8)];
			return { mediaType: 'image/gif', width, height, orientation: 1 };
		}
		if (buf.toString('latin1', 0, 4) === 'RIFF' && buf.toString('latin1', 8, 12) === 'WEBP') {
			return inspectWebp(buf);
		}
		if (buf[0] === 0xff && buf[1] === 0xd8) return inspectJpeg(buf);
	} catch {
		// truncated: reading past the end throws
	}
	return null;
}

function inspectWebp(buf: Buffer): ImageInfo | null {
	const chunk = buf.toString('latin1', 12, 16);
	let width: number;
	let height: number;
	if (chunk === 'VP8 ') {
		width = buf.readUInt16LE(26) & 0x3fff;
		height = buf.readUInt16LE(28) & 0x3fff;
	} else if (chunk === 'VP8L') {
		const bits = buf.readUInt32LE(21);
		width = (bits & 0x3fff) + 1;
		height = ((bits >>> 14) & 0x3fff) + 1;
	} else if (chunk === 'VP8X') {
		width = buf.readUIntLE(24, 3) + 1;
		height = buf.readUIntLE(27, 3) + 1;
	} else return null;
	return { mediaType: 'image/webp', width, height, orientation: 1 };
}

/**
 * Whether the file ends the way its format should. An image the API refuses would stay in the
 * history and fail every later request, so a cut-off download goes through the converter instead.
 */
function isComplete(buf: Buffer, info: ImageInfo): boolean {
	const end = buf.length;
	switch (info.mediaType) {
		case 'image/png':
			return buf.toString('latin1', end - 8, end - 4) === 'IEND';
		case 'image/gif':
			return buf[end - 1] === 0x3b;
		case 'image/webp':
			return buf.readUInt32LE(4) + 8 === end;
		case 'image/jpeg':
			return buf[end - 2] === 0xff && buf[end - 1] === 0xd9;
	}
}

function isStandaloneMarker(marker: number): boolean {
	return marker === 0x01 || (marker >= 0xd0 && marker <= 0xd8);
}

function inspectJpeg(buf: Buffer): ImageInfo | null {
	let orientation = 1;
	let i = 2;
	while (i + 4 <= buf.length && buf[i] === 0xff) {
		const marker = buf[i + 1];
		if (marker === 0xff || isStandaloneMarker(marker)) {
			i += marker === 0xff ? 1 : 2;
			continue;
		}
		if (marker === 0xda || marker === 0xd9) return null; // image data before a frame header
		const length = buf.readUInt16BE(i + 2);
		if (marker === 0xe1 && buf.toString('latin1', i + 4, i + 10) === 'Exif\0\0') {
			orientation = exifOrientation(buf, i + 10, i + 2 + length) ?? 1;
		}
		// Start of frame: C0-CF except DHT (C4), JPG (C8) and DAC (CC).
		if (marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)) {
			const [height, width] = [buf.readUInt16BE(i + 5), buf.readUInt16BE(i + 7)];
			return { mediaType: 'image/jpeg', width, height, orientation };
		}
		i += 2 + length;
	}
	return null;
}

function exifOrientation(buf: Buffer, tiff: number, end: number): number | null {
	const order = buf.toString('latin1', tiff, tiff + 2);
	if (order !== 'II' && order !== 'MM') return null;
	const le = order === 'II';
	const u16 = (at: number) => (le ? buf.readUInt16LE(at) : buf.readUInt16BE(at));
	const u32 = (at: number) => (le ? buf.readUInt32LE(at) : buf.readUInt32BE(at));
	if (u16(tiff + 2) !== 42) return null;
	const ifd = tiff + u32(tiff + 4);
	const entries = u16(ifd);
	for (let n = 0; n < entries; n++) {
		const entry = ifd + 2 + n * 12;
		if (entry + 12 > end) return null;
		if (u16(entry) === 0x0112) return u16(entry + 8);
	}
	return null;
}

/**
 * Drops a JPEG's APP1 segments (EXIF, XMP). They carry GPS positions, and an orientation that
 * browsers apply but the model doesn't, so a JPEG we rotated would show up rotated twice.
 */
function stripJpegMetadata(buf: Buffer): Buffer {
	const parts: Buffer[] = [buf.subarray(0, 2)];
	let i = 2;
	while (i + 4 <= buf.length && buf[i] === 0xff) {
		const marker = buf[i + 1];
		// Start of scan (the rest is image data), or something unusual: keep the rest as it is.
		if (marker === 0xda || marker === 0xff || isStandaloneMarker(marker)) break;
		const end = i + 2 + buf.readUInt16BE(i + 2);
		if (end > buf.length) return buf;
		if (marker !== 0xe1) parts.push(buf.subarray(i, end));
		i = end;
	}
	parts.push(buf.subarray(i));
	return Buffer.concat(parts);
}

// --- converting ---

type Converter = 'sips' | 'magick' | 'convert';

let converter: Converter | null | undefined;

/** sips ships with macOS and opens HEIC; elsewhere ImageMagick, if it's installed. */
function findConverter(): Converter | null {
	if (converter !== undefined) return converter;
	converter = null;
	if (process.platform === 'darwin' && existsSync('/usr/bin/sips')) converter = 'sips';
	else {
		for (const command of ['magick', 'convert'] as const) {
			try {
				execFileSync(command, ['-version'], { stdio: 'ignore', timeout: 10_000 });
				converter = command;
				break;
			} catch {
				// not installed
			}
		}
	}
	return converter;
}

function run(command: string, args: string[]): void {
	try {
		execFileSync(command, args, { stdio: ['ignore', 'ignore', 'pipe'], timeout: 60_000 });
	} catch (err) {
		const stderr = (err as { stderr?: Buffer }).stderr?.toString().trim().split('\n')[0];
		throw new Error(`${command} failed: ${stderr || (err as Error).message}`, { cause: err });
	}
}

/** sips operations that turn an image with this EXIF orientation upright. */
const SIPS_UPRIGHT: Record<number, string[]> = {
	2: ['-f', 'horizontal'],
	3: ['-r', '180'],
	4: ['-f', 'vertical'],
	6: ['-r', '90'],
	8: ['-r', '270']
	// 5 and 7 (rotated and mirrored) don't come out of cameras.
};

/** Writes `output` as an upright `format` image no larger than MAX_EDGE. */
function convert(tool: Converter, input: string, output: string, format: 'jpeg' | 'png'): void {
	if (tool === 'sips') {
		const quality = format === 'jpeg' ? ['-s', 'formatOptions', JPEG_QUALITY] : [];
		run('sips', ['-s', 'format', format, ...quality, input, '--out', output]);
		// sips has no auto-orient, and HEIC sizes can't be read up front: fix both on the result.
		const info = inspectImage(readFileSync(output));
		if (!info) return;
		const ops = [
			...(Math.max(info.width, info.height) > MAX_EDGE ? ['-Z', String(MAX_EDGE)] : []),
			...(SIPS_UPRIGHT[info.orientation] ?? [])
		];
		if (ops.length) run('sips', [...ops, ...quality, output]);
		return;
	}
	const flatten =
		format === 'jpeg' ? ['-background', 'white', '-flatten', '-quality', JPEG_QUALITY] : [];
	run(tool, [
		`${input}[0]`, // first frame or page
		'-auto-orient',
		'-strip',
		'-resize',
		`${MAX_EDGE}x${MAX_EDGE}>`,
		...flatten,
		`${format}:${output}`
	]);
}

export interface PreparedImage {
	data: Buffer;
	info: ImageInfo;
	/** What was done to the file, for the command's output. Null when it's sent as it is. */
	change: string | null;
}

function typeName(info: ImageInfo | null, path: string): string {
	if (info) return info.mediaType.slice('image/'.length).toUpperCase();
	return extname(path).slice(1).toUpperCase() || 'this file';
}

/** Makes a file ready for the model: a supported format, at most MAX_EDGE, upright, small. */
export function prepareImage(path: string): PreparedImage {
	if (!existsSync(path)) throw new Error('no such file');
	if (statSync(path).isDirectory()) throw new Error("it's a folder");
	const original = readFileSync(path);
	const info = inspectImage(original);
	if (
		info &&
		Math.max(info.width, info.height) <= MAX_EDGE &&
		original.length <= REENCODE_ABOVE_BYTES &&
		info.orientation <= 1 &&
		isComplete(original, info)
	) {
		const data = info.mediaType === 'image/jpeg' ? stripJpegMetadata(original) : original;
		return { data, info, change: null };
	}
	if (original.toString('latin1', 0, 5) === '%PDF-') {
		throw new Error("it's a PDF. btw view shows images: turn the pages you need into images first");
	}
	const tool = findConverter();
	if (!tool) {
		throw new Error(
			`it has to be converted (to JPEG or PNG, at most ${MAX_EDGE} px) and neither sips nor ImageMagick is available. Convert it first`
		);
	}

	const work = mkdtempSync(join(tmpdir(), 'btw-convert-'));
	try {
		// Screenshots and drawings stay lossless unless that's too big; photos become JPEG.
		const formats: ('png' | 'jpeg')[] =
			info && info.mediaType !== 'image/jpeg' ? ['png', 'jpeg'] : ['jpeg'];
		let data: Buffer | null = null;
		for (const format of formats) {
			const output = join(work, `image.${format}`);
			convert(tool, path, output, format);
			data = existsSync(output) ? readFileSync(output) : null;
			if (data && data.length <= REENCODE_ABOVE_BYTES) break;
		}
		const result = data && inspectImage(data);
		if (!data || !result) throw new Error(`couldn't convert ${typeName(info, path)} to an image`);
		if (Math.max(result.width, result.height) > MAX_EDGE || data.length > MAX_IMAGE_BYTES) {
			throw new Error('still too big after shrinking it');
		}
		const changes: string[] = [];
		if (!info || info.mediaType !== result.mediaType) {
			changes.push(`converted from ${typeName(info, path)}`);
		}
		if (info && Math.max(info.width, info.height) > MAX_EDGE) {
			changes.push(`shrunk from ${info.width}×${info.height}`);
		}
		if (info && info.orientation > 1) changes.push('turned upright');
		return {
			data: result.mediaType === 'image/jpeg' ? stripJpegMetadata(data) : data,
			info: { ...result, orientation: 1 },
			change: changes.join(', ') || null
		};
	} finally {
		rmSync(work, { recursive: true, force: true });
	}
}

// --- handing images from `btw view` to the gateway ---

const LIMITS_FILE = 'limits.json';
const MANIFEST_FILE = 'manifest.jsonl';

/** Images already in a conversation. Each one is resent with every request. */
export interface ImageUse {
	count: number;
	/** Base64 characters. */
	bytes: number;
}

interface ViewEntry {
	/** File name inside the view folder. */
	file: string;
	/** The path as it was given to `btw view`. */
	name: string;
	/** Base64 size, for `btw view`'s own count. The gateway measures the file itself. */
	bytes?: number;
}

function base64Length(bytes: number): number {
	return 4 * Math.ceil(bytes / 3);
}

export function imageUse(messages: Anthropic.MessageParam[]): ImageUse {
	const use: ImageUse = { count: 0, bytes: 0 };
	const add = (block: { type: string }) => {
		if (block.type !== 'image') return;
		const { source } = block as Anthropic.ImageBlockParam;
		use.count++;
		if (source.type === 'base64') use.bytes += source.data.length;
	};
	for (const message of messages) {
		if (typeof message.content === 'string') continue;
		for (const block of message.content) {
			add(block);
			if (block.type === 'tool_result' && Array.isArray(block.content)) block.content.forEach(add);
		}
	}
	return use;
}

/** Gateway, before a command runs: its view folder, with what the conversation has room for. */
export function createViewDir(used: ImageUse): string {
	const dir = mkdtempSync(join(tmpdir(), 'btw-view-'));
	const limits: ImageUse = {
		count: MAX_CONVERSATION_IMAGES - used.count,
		bytes: MAX_CONVERSATION_IMAGE_BYTES - used.bytes
	};
	writeFileSync(join(dir, LIMITS_FILE), JSON.stringify(limits));
	return dir;
}

function readManifest(dir: string): ViewEntry[] {
	let text: string;
	try {
		text = readFileSync(join(dir, MANIFEST_FILE), 'utf8');
	} catch {
		return [];
	}
	return text.split('\n').flatMap((line) => {
		try {
			const entry = JSON.parse(line) as ViewEntry;
			return typeof entry.file === 'string' && typeof entry.name === 'string' ? [entry] : [];
		} catch {
			return [];
		}
	});
}

/** A limit that also stops every later image of the same command. */
export class ViewLimitError extends Error {}

const CONVERSATION_FULL =
	'this conversation already holds as many images as it can (each step resends all of them). Say what you need to in words, or suggest a new conversation for more images';

/** `btw view`, for one file: prepares it and leaves it for the gateway. Returns a line to print. */
export function viewImage(path: string, dir: string): string {
	const limits = JSON.parse(readFileSync(join(dir, LIMITS_FILE), 'utf8')) as ImageUse;
	const earlier = readManifest(dir);
	if (earlier.length >= MAX_IMAGES_PER_COMMAND) {
		throw new ViewLimitError(
			`at most ${MAX_IMAGES_PER_COMMAND} images per command. Look at the rest in another command`
		);
	}
	if (earlier.length >= limits.count) throw new ViewLimitError(CONVERSATION_FULL);
	const image = prepareImage(path);
	const bytes = base64Length(image.data.length);
	const total = earlier.reduce((sum, entry) => sum + (entry.bytes ?? 0), bytes);
	if (total > limits.bytes) throw new ViewLimitError(CONVERSATION_FULL);

	const file = `${randomUUID()}.${image.info.mediaType.slice('image/'.length)}`;
	writeFileSync(join(dir, file), image.data);
	const entry: ViewEntry = { file, name: path, bytes };
	appendFileSync(join(dir, MANIFEST_FILE), JSON.stringify(entry) + '\n');
	const { width, height } = image.info;
	const details = [`${width}×${height} ${typeName(image.info, path)}`, image.change].filter(
		Boolean
	);
	return `Attached ${path} (${details.join(', ')}).`;
}

/**
 * Gateway, after the command exited: the images `btw view` left, as content for the tool_result,
 * each after a text line naming it. Checks the limits again and adds what it attached to `used`.
 */
export function collectViewedImages(
	dir: string,
	used: ImageUse
): (Anthropic.TextBlockParam | Anthropic.ImageBlockParam)[] {
	const blocks: (Anthropic.TextBlockParam | Anthropic.ImageBlockParam)[] = [];
	let attached = 0;
	for (const entry of readManifest(dir)) {
		if (basename(entry.file) !== entry.file) continue;
		let data: Buffer;
		try {
			data = readFileSync(join(dir, entry.file));
		} catch {
			continue;
		}
		const info = inspectImage(data);
		const bytes = base64Length(data.length);
		const problem = !info
			? 'not an image'
			: Math.max(info.width, info.height) > MAX_EDGE || data.length > MAX_IMAGE_BYTES
				? 'too big'
				: attached >= MAX_IMAGES_PER_COMMAND ||
					  used.count >= MAX_CONVERSATION_IMAGES ||
					  used.bytes + bytes > MAX_CONVERSATION_IMAGE_BYTES
					? 'over the image limit'
					: null;
		if (!info || problem) {
			blocks.push({ type: 'text', text: `Not attached: ${entry.name} (${problem}).` });
			continue;
		}
		blocks.push(
			{ type: 'text', text: `Image: ${entry.name}` },
			{
				type: 'image',
				source: { type: 'base64', media_type: info.mediaType, data: data.toString('base64') }
			}
		);
		attached++;
		used.count++;
		used.bytes += bytes;
	}
	return blocks;
}
