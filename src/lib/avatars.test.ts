import { readFileSync } from 'node:fs';
import { AVATARS } from '@btw/core/avatars';
import { describe, expect, it } from 'vitest';
import { avatarFavicon, parseAvatarColors } from './avatars';

// Read from disk: Vitest leaves CSS imports empty.
const css = readFileSync(new URL('../routes/layout.css', import.meta.url), 'utf8');
const AVATAR_COLORS = parseAvatarColors(css);

/** A variable's light (`:root`) and dark (`.dark`) values in layout.css. */
function themed(name: string): [string, string] {
	const values = [...css.matchAll(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`, 'gi'))];
	return [values[0][1], values[1][1]];
}

function luminance(hex: string): number {
	const [r, g, b] = [1, 3, 5].map((i) => {
		const c = parseInt(hex.slice(i, i + 2), 16) / 255;
		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	});
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

describe('avatar colors', () => {
	it('has a light and a dark color for every avatar', () => {
		for (const avatar of AVATARS) {
			expect(AVATAR_COLORS[avatar].light, avatar).toMatch(/^#[0-9a-f]{6}$/);
			expect(AVATAR_COLORS[avatar].dark, avatar).toMatch(/^#[0-9a-f]{6}$/);
		}
	});

	it('stands out 3:1 from every surface an avatar sits on, in both themes', () => {
		const surfaces = ['background', 'sidebar', 'bubble', 'popover'].map(themed);
		for (const avatar of AVATARS) {
			const { light, dark } = AVATAR_COLORS[avatar];
			for (const [lightSurface, darkSurface] of surfaces) {
				expect(contrast(light, lightSurface), `${avatar} on ${lightSurface}`).toBeGreaterThan(3);
				expect(contrast(dark, darkSurface), `${avatar} on ${darkSurface}`).toBeGreaterThan(3);
			}
		}
	});

	it('makes a favicon that follows the system theme', () => {
		const svg = decodeURIComponent(
			avatarFavicon('comet', AVATAR_COLORS.comet).replace('data:image/svg+xml,', '')
		);
		expect(svg).toMatch(/^<svg xmlns="http:\/\/www.w3.org\/2000\/svg" viewBox="0 0 24 24"/);
		expect(svg).toContain(`svg{fill:${AVATAR_COLORS.comet.light}}`);
		expect(svg).toContain(
			`@media (prefers-color-scheme:dark){svg{fill:${AVATAR_COLORS.comet.dark}}}`
		);
	});
});
