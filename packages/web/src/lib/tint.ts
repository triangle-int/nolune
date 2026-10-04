import { avatarTint as tintOf, type Tint } from '@nolune/core/tint';
import type { Avatar } from '@nolune/core/avatars';
import css from '../routes/layout.css?raw';

export type { Tint } from '@nolune/core/tint';

/**
 * The greys tinted with the avatar's hue, from the greys and avatar colors in layout.css.
 * (Vitest leaves CSS imports empty; its tests pass the file.)
 */
export function avatarTint(avatar: Avatar, stylesheet = css): Tint {
	return tintOf(avatar, stylesheet);
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
