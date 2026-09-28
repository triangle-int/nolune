import type { Avatar } from '@nolune/core/avatars';
import css from '../routes/layout.css?raw';
import { parseAvatarColors } from './avatars';

// --- OKLab (https://bottosson.github.io/posts/oklab/), for hues that look even across colors ---

type Vec = [number, number, number];

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const fromLinear = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

function linear(hex: string): Vec {
	return [1, 3, 5].map((i) => toLinear(parseInt(hex.slice(i, i + 2), 16) / 255)) as Vec;
}

function oklab([r, g, b]: Vec): Vec {
	const [l, m, s] = [
		0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b,
		0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b,
		0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
	].map(Math.cbrt);
	return [
		0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
		1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
		0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
	];
}

/** Linear sRGB, which can fall outside 0–1 when the color doesn't fit. */
function linearRgb([L, a, b]: Vec): Vec {
	const [l, m, s] = [
		L + 0.3963377774 * a + 0.2158037573 * b,
		L - 0.1055613458 * a - 0.0638541728 * b,
		L - 0.0894841775 * a - 1.291485548 * b
	].map((c) => c ** 3);
	return [
		4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
		-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
		-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s
	];
}

/** Relative luminance, which contrast ratios are worked out from. */
const luminance = ([r, g, b]: Vec) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

function hex(rgb: Vec): string {
	return (
		'#' +
		rgb
			.map((c) => Math.round(Math.min(1, Math.max(0, fromLinear(c))) * 255))
			.map((c) => c.toString(16).padStart(2, '0'))
			.join('')
	);
}

/** The color's hue angle in OKLCH, in radians. */
function hue(hex: string): number {
	const [, a, b] = oklab(linear(hex));
	return Math.atan2(b, a);
}

/**
 * A grey given `chroma` of a hue, with the grey's luminance, so text and avatars keep their
 * contrast on it. White has no room for a hue: it steps down until the color fits sRGB.
 */
function tinted(grey: string, angle: number, chroma: number): string {
	const [a, b] = [chroma * Math.cos(angle), chroma * Math.sin(angle)];
	const target = luminance(linear(grey));
	let [lo, hi] = [0, 1];
	while (hi - lo > 1e-6) {
		const mid = (lo + hi) / 2;
		if (luminance(linearRgb([mid, a, b])) < target) lo = mid;
		else hi = mid;
	}
	for (let L = lo; ; L -= 0.001) {
		const rgb = linearRgb([L, a, b]);
		if (rgb.every((c) => c > -1e-4 && c < 1 + 1e-4)) return hex(rgb);
	}
}

// --- the tint ---

/**
 * The greys a profile's pages tint with its avatar's hue, and how much (OKLCH chroma). Borders,
 * inputs and the dark accent are see-through, so they pick it up from what's under them. Light
 * cards, menus and the composer stay white, a step above the tinted page.
 */
const TINT: Record<'light' | 'dark', Record<string, number>> = {
	light: {
		background: 0.005,
		sidebar: 0.008,
		'sidebar-accent': 0.012,
		secondary: 0.01,
		muted: 0.01,
		accent: 0.012,
		bubble: 0.01
	},
	dark: {
		background: 0.01,
		card: 0.011,
		popover: 0.012,
		sidebar: 0.009,
		'sidebar-accent': 0.011,
		secondary: 0.012,
		muted: 0.012,
		bubble: 0.012,
		composer: 0.012
	}
};

/** A variable's light (`:root`) and dark (`.dark`) values in a stylesheet. */
function themed(stylesheet: string, name: string): { light: string; dark: string } {
	const [light, dark] = [
		...stylesheet.matchAll(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})\\b`, 'gi'))
	].map((match) => match[1]);
	return { light, dark };
}

export type Tint = Record<'light' | 'dark', Record<string, string>>;

/**
 * The greys tinted with the avatar's hue, from the greys and avatar colors in layout.css.
 * (Vitest leaves CSS imports empty; its tests pass the file.)
 */
export function avatarTint(avatar: Avatar, stylesheet = css): Tint {
	const colors = parseAvatarColors(stylesheet)[avatar];
	const theme = (mode: 'light' | 'dark') => {
		const angle = hue(colors[mode]);
		return Object.fromEntries(
			Object.entries(TINT[mode]).map(([name, chroma]) => [
				name,
				tinted(themed(stylesheet, name)[mode], angle, chroma)
			])
		);
	};
	return { light: theme('light'), dark: theme('dark') };
}

/**
 * The `<style>` element that tints the page. Both selectors outrank layout.css's `:root` and
 * `.dark` wherever the two end up in the page. The light one skips the dark theme: a grey tinted
 * only in light, like the accent, would otherwise outrank `.dark` and put light greys under dark
 * mode's light text. (Built here: Svelte's preprocessor would take a `<style>` written in a
 * component for the component's own.)
 */
export function tintStyle({ light, dark }: Tint): string {
	const vars = (colors: Record<string, string>) =>
		Object.entries(colors)
			.map(([name, color]) => `--${name}:${color};`)
			.join('');
	return `<style>:root:not(.dark){${vars(light)}}.dark.dark{${vars(dark)}}</style>`;
}
