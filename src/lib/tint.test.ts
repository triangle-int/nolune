import { readFileSync } from 'node:fs';
import { AVATARS } from '@btw/core/avatars';
import { describe, expect, it } from 'vitest';
import { parseAvatarColors } from './avatars';
import { avatarTint, tintStyle } from './tint';

// Read from disk: Vitest leaves CSS imports empty.
const css = readFileSync(new URL('../routes/layout.css', import.meta.url), 'utf8');
const AVATAR_COLORS = parseAvatarColors(css);
const TINTS = AVATARS.map((avatar) => avatarTint(avatar, css));
const MODES = ['light', 'dark'] as const;

/** A variable's light (`:root`) and dark (`.dark`) values in layout.css. */
function themed(name: string): { light: string; dark: string } {
	const [light, dark] = [...css.matchAll(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})\\b`, 'gi'))].map(
		(match) => match[1]
	);
	return { light, dark };
}

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

function luminance(hex: string): number {
	const [r, g, b] = channels(hex).map((c) => {
		c /= 255;
		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	});
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

describe('avatar tint', () => {
	it('tints the greys in both themes', () => {
		for (const tint of TINTS) {
			for (const mode of MODES) {
				expect(Object.keys(tint[mode])).toContain('background');
				for (const [name, color] of Object.entries(tint[mode])) {
					expect(color, name).toMatch(/^#[0-9a-f]{6}$/);
					expect(color, name).not.toBe(themed(name)[mode]);
				}
			}
		}
	});

	it("leans toward the avatar's hue", () => {
		const [r, , b] = channels(avatarTint('comet', css).dark.background);
		expect(b).toBeGreaterThan(r);
		const [r2, , b2] = channels(avatarTint('campfire', css).dark.background);
		expect(r2).toBeGreaterThan(b2);
	});

	it('keeps text as readable as on the plain greys', () => {
		for (const tint of TINTS) {
			for (const mode of MODES) {
				const text = themed('foreground')[mode];
				for (const [name, color] of Object.entries(tint[mode])) {
					const plain = contrast(text, themed(name)[mode]);
					expect(contrast(text, color), name).toBeGreaterThan(plain * 0.95);
				}
			}
		}
	});

	it("keeps every avatar 3:1 from every profile's surfaces, as the profile switcher shows them", () => {
		for (const tint of TINTS) {
			for (const mode of MODES) {
				for (const name of ['background', 'sidebar', 'bubble', 'popover']) {
					const surface = tint[mode][name] ?? themed(name)[mode];
					for (const avatar of AVATARS) {
						const color = AVATAR_COLORS[avatar][mode];
						expect(contrast(color, surface), `${avatar} on ${surface}`).toBeGreaterThan(3);
					}
				}
			}
		}
	});

	it('overrides layout.css in both themes', () => {
		const style = tintStyle(avatarTint('comet', css));
		expect(style).toMatch(/^<style>:root:root\{--background:#[0-9a-f]{6};.*\}<\/style>$/);
		expect(style).toMatch(/\}\.dark\.dark\{--background:#[0-9a-f]{6};/);
	});
});
