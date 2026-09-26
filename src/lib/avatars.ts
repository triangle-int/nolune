import { AVATARS, avatarSvg, type Avatar } from '@btw/core/avatars';
import css from '../routes/layout.css?raw';

export type AvatarColors = Record<Avatar, { light: string; dark: string }>;

/** The `--avatar-*` colors in a stylesheet: light (`:root`) first, then dark (`.dark`). */
export function parseAvatarColors(stylesheet: string): AvatarColors {
	return Object.fromEntries(
		AVATARS.map((avatar) => {
			const [light, dark] = [
				...stylesheet.matchAll(new RegExp(`--avatar-${avatar}:\\s*(#[0-9a-f]{3,8})`, 'gi'))
			].map((match) => match[1]);
			return [avatar, { light, dark }];
		})
	) as AvatarColors;
}

/**
 * Each avatar's colors as layout.css defines them, for pictures that can't use CSS variables.
 * (Vitest leaves CSS imports empty; its tests parse the file themselves.)
 */
export const AVATAR_COLORS = parseAvatarColors(css);

/**
 * The avatar as a tab icon. The tab strip follows the system's theme rather than the page's, so the
 * icon picks its color the same way.
 */
export function avatarFavicon(avatar: Avatar, { light, dark } = AVATAR_COLORS[avatar]): string {
	const svg = avatarSvg(avatar, light).replace(
		/<svg[^>]*>/,
		(open) =>
			`${open}<style>svg{fill:${light}}@media (prefers-color-scheme:dark){svg{fill:${dark}}}</style>`
	);
	return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}
