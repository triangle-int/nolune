import type { ConversationCursor } from '@nolune/core';

/**
 * The JSON that apps of their own, like nolune for iOS, read instead of the pages (DESIGN.md, "The
 * API for apps"). `/api/version` says the level; it goes up with any change those apps would
 * notice, so an app can tell what a family's nolune serves, whichever version it runs.
 */
export const API_LEVEL = 1;

/** What an app can show natively from this nolune, as `/api/version` lists it signed in. */
export const CAPABILITIES = ['chats', 'notifications', 'transcript'] as const;

/** A page of chats at most, and when the app doesn't say. */
export const MAX_CHATS = 100;
export const DEFAULT_CHATS = 50;

/** The `after` an app passes back for the next page: opaque to it, `<updatedAt ms>.<id>` here. */
export function encodeCursor(chat: ConversationCursor): string {
	return `${chat.updatedAt.getTime()}.${chat.id}`;
}

export function decodeCursor(text: string): ConversationCursor | null {
	const match = /^(\d+)\.([\w-]+)$/.exec(text);
	if (!match) return null;
	return { updatedAt: new Date(Number(match[1])), id: match[2] };
}

/** One chat as the list sends it. Its title is empty until it has one: apps show their own words. */
export function chatSummary(
	chat: {
		id: string;
		title: string;
		presetName: string;
		folderId: string | null;
		updatedAt: Date;
	},
	running: ReadonlySet<string>
) {
	return {
		id: chat.id,
		title: chat.title,
		presetName: chat.presetName,
		folderId: chat.folderId,
		updatedAt: chat.updatedAt.getTime(),
		running: running.has(chat.id)
	};
}
