import type { Avatar } from '@nolune/core/avatars';

/** The app's `--avatar-*` colors (src/routes/layout.css): light for its light theme, dark for dark. */
export const AVATAR_COLORS: Record<Avatar, { light: string; dark: string }> = {
	probe: { light: '#039a80', dark: '#70e0c4' },
	campfire: { light: '#d66a03', dark: '#fe9042' },
	lantern: { light: '#ad7f00', dark: '#fee57f' },
	planet: { light: '#8c74f7', dark: '#a293fd' },
	quantum: { light: '#0b9b6d', dark: '#85ebbd' },
	comet: { light: '#3490c3', dark: '#8ed2fd' },
	moon: { light: '#9a8363', dark: '#fcedd2' },
	satellite: { light: '#f44959', dark: '#fd767b' }
};
