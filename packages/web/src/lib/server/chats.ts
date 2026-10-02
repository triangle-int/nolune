import {
	AttachmentError,
	EFFORTS,
	chatCommandChoice,
	commandMode,
	createConversation,
	deleteConversation,
	findUploads,
	getFolder,
	getPreset,
	isCommandMode,
	sendMessage,
	stopConversation,
	type Conversation,
	type Effort,
	type Profile
} from '@nolune/core';
import type { Messages } from '$lib/i18n';

/** A chat to start, as the new chat's page and the JSON for apps (/api/p/<slug>/chats) ask. */
export interface NewChat {
	presetId: string;
	effort: string;
	/** Its first message, if any: a chat can start empty, or with files only. */
	text: string;
	uploads: string[];
	folderId: string | null;
	/** Left out (the folder's page), the chat goes by Models & keys. */
	commands?: string;
}

export type Started =
	{ conversation: Conversation } | { status: 400 | 403; message: string; conversation?: undefined };

/**
 * Starts a conversation, in a folder if one was picked, with its first message when one was typed
 * or files attached. Nothing is made unless all of it checks out.
 */
export async function startChat(
	user: { id: string; name: string; isAdmin?: boolean | null },
	profile: Profile,
	chat: NewChat,
	m: Messages
): Promise<Started> {
	const effort = chat.effort as Effort;
	if (!getPreset(chat.presetId)) return { status: 400, message: m.newChat.pickModel };
	if (!EFFORTS.includes(effort)) return { status: 400, message: m.newChat.pickEffort };
	if (chat.folderId && !getFolder(profile.id, chat.folderId)) {
		return { status: 400, message: m.newChat.folderGone };
	}
	const commands = chat.commands || commandMode();
	if (!isCommandMode(commands)) return { status: 400, message: 'Unknown command mode' };
	const commandChoice = chatCommandChoice(commands);
	if (commandChoice === 'unrestricted' && !user.isAdmin) {
		return { status: 403, message: m.commandMode.adminsOnly };
	}
	try {
		// Checked before the conversation exists, so a stale file doesn't leave an empty chat.
		findUploads(profile.id, user.id, chat.uploads);
	} catch (err) {
		if (err instanceof AttachmentError) return { status: 400, message: err.message };
		throw err;
	}
	const conversation = createConversation({
		profile,
		presetId: chat.presetId,
		userId: user.id,
		effort,
		folderId: chat.folderId,
		commandMode: commandChoice
	});
	if (chat.text || chat.uploads.length) {
		await sendMessage(conversation.id, { id: user.id, name: user.name }, chat.text, chat.uploads);
	}
	return { conversation };
}

/** Deletes the chat for everyone. Nothing keeps working, or reports back, for a chat that's gone. */
export function removeChat(id: string, byName: string): void {
	stopConversation(id, byName);
	deleteConversation(id);
}
