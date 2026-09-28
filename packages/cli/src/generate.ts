import { mkdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { extname, isAbsolute, join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import {
	IMAGE_BACKGROUNDS,
	IMAGE_FORMATS,
	MAX_IMAGE_COUNT,
	configuredImageModel,
	generateImages,
	inspectImage,
	parseImageSize,
	type ImageBackground,
	type ImageFormat
} from '@nolune/core';
import type { Io } from './io.ts';

/** Read when shown: the gateway runs for days, and the image model can change meanwhile. */
export function generateHelp(): string {
	return `Pictures (model ${configuredImageModel()}; change it with \`nolune config set image-model\`)
  nolune generate image <PROMPT | -> [--image FILE]... [--size square|portrait|landscape|auto|WxH]
                        [--quality Q] [--background auto|transparent|opaque] [--format png|jpeg|webp]
                        [--count N] [--model PROVIDER/MODEL] [--out DIR|FILE] [--dry-run]
      make pictures from a prompt, or change the --image ones; \`-\` reads the prompt from stdin`;
}

const OPTIONS = {
	image: { type: 'string', short: 'i', multiple: true },
	size: { type: 'string' },
	quality: { type: 'string' },
	background: { type: 'string' },
	format: { type: 'string' },
	count: { type: 'string', short: 'n' },
	model: { type: 'string' },
	out: { type: 'string', short: 'o' },
	'dry-run': { type: 'boolean' }
} as const;

function oneOf<T extends string>(value: string | undefined, allowed: readonly T[], flag: string) {
	if (value === undefined) return undefined;
	const v = value.trim().toLowerCase();
	if (!(allowed as readonly string[]).includes(v)) {
		throw new Error(`--${flag} is one of ${allowed.join(', ')}.`);
	}
	return v as T;
}

function expandHome(path: string): string {
	if (path === '~') return homedir();
	return path.startsWith('~/') ? join(homedir(), path.slice(2)) : path;
}

/** The first few words of the prompt, for the file name. */
function slug(text: string): string {
	return (
		text
			.toLowerCase()
			.normalize('NFKD')
			.replace(/[^a-z0-9\s-]/g, '')
			.trim()
			.split(/[\s-]+/)
			.slice(0, 5)
			.join('-') || 'image'
	);
}

function stamp(now: Date): string {
	const p = (n: number) => String(n).padStart(2, '0');
	return `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
}

/** Where each picture goes: `--out` as a file (one picture) or a folder, else `images/`. */
function outputPaths(io: Io, out: string | undefined, count: number, base: string, format: string) {
	const ext = format === 'jpeg' ? 'jpg' : format;
	let dir: string;
	if (out && /\.(png|jpe?g|webp)$/i.test(out)) {
		const file = resolve(io.cwd, expandHome(out));
		if (count === 1) return { dir: resolve(file, '..'), files: [file] };
		dir = resolve(file, '..');
		base = file.slice(dir.length + 1, -extname(file).length);
	} else if (out) {
		dir = resolve(io.cwd, expandHome(out));
	} else {
		// In the agent's commands: the profile's images folder, so the family finds them later.
		const profileDir = io.env.NOLUNE_PROFILE_DIR;
		dir = profileDir && isAbsolute(profileDir) ? join(profileDir, 'images') : io.cwd;
	}
	const files = Array.from({ length: count }, (_, i) =>
		join(dir, `${base}${count > 1 ? `-${i + 1}` : ''}.${ext}`)
	);
	return { dir, files };
}

function formatBytes(bytes: number): string {
	return bytes >= 1024 * 1024
		? `${(bytes / (1024 * 1024)).toFixed(1)} MB`
		: `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

async function image(io: Io, args: string[]): Promise<void> {
	const { values, positionals } = parseArgs({ args, allowPositionals: true, options: OPTIONS });
	let prompt = positionals.join(' ').trim();
	if (prompt === '-') prompt = (await io.readStdin()).trim();
	if (!prompt) throw new Error('say what to make: nolune generate image "<prompt>"');
	const inputs = (values.image ?? []).map((path) => resolve(io.cwd, expandHome(path)));
	const count = values.count === undefined ? 1 : Number(values.count);
	if (!Number.isInteger(count) || count < 1 || count > MAX_IMAGE_COUNT) {
		throw new Error(`--count is between 1 and ${MAX_IMAGE_COUNT}.`);
	}
	const background = oneOf<ImageBackground>(values.background, IMAGE_BACKGROUNDS, 'background');
	const format = oneOf<ImageFormat>(values.format, IMAGE_FORMATS, 'format');
	const size = values.size ? parseImageSize(values.size) : 'auto';
	const quality = values.quality?.trim().toLowerCase() || undefined;

	if (values['dry-run']) {
		const sizeText = typeof size === 'object' ? `${size.width}x${size.height}` : size;
		io.log(`Model: ${values.model?.trim() || configuredImageModel()}`);
		io.log(
			`Size: ${sizeText}, quality: ${quality ?? 'default'}, background: ${background ?? 'default'}, format: ${format ?? 'png'}, count: ${count}`
		);
		if (inputs.length) io.log(`Images: ${inputs.join(', ')}`);
		io.log(`Prompt:\n${prompt}`);
		return;
	}

	const result = await generateImages({
		prompt,
		images: inputs,
		model: values.model,
		size,
		quality,
		background,
		format,
		count,
		signal: io.signal,
		onStart: (model) => {
			const from = inputs.length
				? ` from ${inputs.length} image${inputs.length === 1 ? '' : 's'}`
				: '';
			const what = count === 1 ? 'a picture' : `${count} pictures`;
			io.log(`Making ${what} with ${model}${from}. This can take a minute or two.`);
		}
	});

	const base = `${stamp(new Date())}-${slug(prompt)}`;
	const { dir, files } = outputPaths(
		io,
		values.out,
		result.images.length,
		base,
		result.images[0].format
	);
	mkdirSync(dir, { recursive: true });
	for (const [i, picture] of result.images.entries()) {
		writeFileSync(files[i], picture.data);
		const info = inspectImage(picture.data);
		const details = [
			info ? `${info.width}×${info.height}` : null,
			picture.format.toUpperCase(),
			formatBytes(picture.data.length)
		].filter(Boolean);
		io.log(`Saved ${files[i]} (${details.join(', ')})`);
	}
}

export async function generateCommand(
	io: Io,
	action: string | undefined,
	args: string[]
): Promise<void> {
	if (action === 'image') return image(io, args);
	throw new Error('usage: nolune generate image <prompt> [options]. See `nolune help`.');
}
