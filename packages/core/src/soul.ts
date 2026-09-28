import { randomUUID } from 'node:crypto';
import { lstatSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { cutAtLine } from './memory.ts';
import { profileSoulFile } from './paths.ts';

/*
 * A profile's soul: who nolune is for its family (character, values, tone, boundaries), in
 * `soul.md` in the profile folder. It opens the system prompt of every chat, which is built again
 * when it changes. The family edits it in the profile's settings, and nolune changes it itself with
 * `nolune soul`.
 */

/** It is in every prompt, so it stays a page, not a manual. */
export const MAX_SOUL_CHARS = 4_000;

/** A refused change. The message says what to do instead. */
export class SoulError extends Error {}

/** The whole soul, as people edit it; '' without one. */
export function readSoulFile(slug: string): string {
	try {
		const file = profileSoulFile(slug);
		// No links, and nothing only a person could have copied in.
		const stat = lstatSync(file);
		if (stat.isFile() && stat.size <= 1_000_000) return readFileSync(file, 'utf8').trim();
	} catch {
		// No soul yet.
	}
	return '';
}

/**
 * The soul as the system prompt has it: '' without one, and cut at a line (`cut`) when it grew
 * past the limit some other way (an editor).
 */
export function readSoul(slug: string): { text: string; cut: boolean } {
	return cutAtLine(readSoulFile(slug), MAX_SOUL_CHARS);
}

/** Replaces the soul; empty text removes it. Returns what was saved. */
export function writeSoul(slug: string, text: string): string {
	const clean = text.replace(/\r\n?/g, '\n').trim();
	if (clean.length > MAX_SOUL_CHARS) {
		throw new SoulError(
			`The soul would be ${clean.length} characters; it can have at most ${MAX_SOUL_CHARS}, because it goes into every chat. Keep who nolune is and how it behaves; facts about the family belong in memory.`
		);
	}
	const file = profileSoulFile(slug);
	if (!clean) {
		rmSync(file, { force: true });
		return '';
	}
	// Readers never see a half-written file.
	mkdirSync(dirname(file), { recursive: true, mode: 0o700 });
	const temp = join(dirname(file), `.soul-${randomUUID()}.tmp`);
	try {
		writeFileSync(temp, `${clean}\n`, { mode: 0o600 });
		renameSync(temp, file);
	} finally {
		rmSync(temp, { force: true });
	}
	return clean;
}
