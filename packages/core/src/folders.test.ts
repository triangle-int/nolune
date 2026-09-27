import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { describe, expect, it } from 'vitest';
import { toAnthropicMessages } from './anthropic.ts';
import { createUpload } from './attachments.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	getConversation,
	insertQueued,
	listConversations,
	requestMessages
} from './conversations.ts';
import {
	FolderError,
	MAX_FOLDER_INSTRUCTIONS,
	addFolderFiles,
	createFolder,
	deleteFolder,
	folderDir,
	getFolder,
	listFolderFiles,
	listFolders,
	moveConversation,
	removeFolderFile,
	renameFolder,
	setFolderInstructions
} from './folders.ts';
import { pruneMedia, blobPath } from './media.ts';
import { paths } from './paths.ts';
import { withCurrentContext } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

function upload(profileId: string, userId: string, name: string, data: string) {
	return createUpload({ profileId, userId, name, body: Readable.from([Buffer.from(data)]) });
}

function reply(conversationId: string, content: unknown[]) {
	return appendRow({
		conversationId,
		role: 'assistant',
		kind: 'assistant',
		content: JSON.stringify(content)
	});
}

function say(conversationId: string, user: { id: string; name: string }, text: string) {
	insertQueued({ conversationId, senderId: user.id, senderName: user.name, text });
	commitQueuedRows(conversationId);
}

describe('folders', () => {
	it('get a folder on disk named after them, unique in the profile', () => {
		const { user, profile } = makeFamily();
		const trip = createFolder({ profile, name: '  Trip to  Japan ', userId: user.id });
		const again = createFolder({ profile, name: 'Trip to Japan', userId: user.id });
		expect(trip).toMatchObject({ name: 'Trip to Japan', slug: 'trip-to-japan', instructions: '' });
		expect(again.slug).toBe('trip-to-japan-2');
		expect(existsSync(folderDir(profile.slug, trip.slug))).toBe(true);
		expect(listFolders(profile.id).map((f) => f.id)).toEqual([trip.id, again.id]);

		renameFolder(trip.id, 'Japan');
		expect(getFolder(profile.id, trip.id)).toMatchObject({ name: 'Japan', slug: 'trip-to-japan' });
	});

	it('belong to their profile', () => {
		const { user, profile } = makeFamily();
		const other = makeFamily('Ben');
		const found = createFolder({ profile, name: 'School', userId: user.id });
		expect(getFolder(other.profile.id, found.id)).toBeUndefined();
		expect(() => moveConversation(other.profile.id, 'any', found.id)).toThrow(FolderError);
	});

	it('refuse empty names and overlong instructions', () => {
		const { user, profile } = makeFamily();
		expect(() => createFolder({ profile, name: '  ', userId: user.id })).toThrow(FolderError);
		const found = createFolder({ profile, name: 'School', userId: user.id });
		expect(() => setFolderInstructions(found.id, 'x'.repeat(MAX_FOLDER_INSTRUCTIONS + 1))).toThrow(
			FolderError
		);
	});

	it('keep files as a copy for the agent and the original for the page', async () => {
		const { user, profile } = makeFamily();
		const found = createFolder({ profile, name: 'Trip', userId: user.id });
		const a = await upload(profile.id, user.id, 'plan.txt', 'day 1: Tokyo');
		const b = await upload(profile.id, user.id, 'plan.txt', 'day 2: Kyoto');
		const same = await upload(profile.id, user.id, 'plan.txt', 'day 1: Tokyo');
		const added = await addFolderFiles({
			profile,
			folder: found,
			userId: user.id,
			uploadIds: [a.id, b.id]
		});
		const dir = folderDir(profile.slug, found.slug);
		expect(added.map((f) => [f.name, f.path])).toEqual([
			['plan.txt', join(dir, 'plan.txt')],
			['plan (2).txt', join(dir, 'plan (2).txt')]
		]);
		expect(readFileSync(join(dir, 'plan (2).txt'), 'utf8')).toBe('day 2: Kyoto');

		// The same file again is already there.
		await addFolderFiles({ profile, folder: found, userId: user.id, uploadIds: [same.id] });
		expect(listFolderFiles(found.id)).toHaveLength(2);
		// Uploads are used up.
		await expect(
			addFolderFiles({ profile, folder: found, userId: user.id, uploadIds: [a.id] })
		).rejects.toThrow(FolderError);

		pruneMedia();
		expect(existsSync(blobPath(a.sha256))).toBe(true);

		expect(removeFolderFile(found.id, added[0].id)).toBe(true);
		expect(existsSync(added[0].path)).toBe(false);
		// A copy someone changed since is left alone.
		writeFileSync(added[1].path, 'changed');
		removeFolderFile(found.id, added[1].id);
		expect(existsSync(added[1].path)).toBe(true);
		expect(listFolderFiles(found.id)).toEqual([]);
	});

	it('when deleted, move their files to the trash and their chats back to the list', () => {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const found = createFolder({ profile, name: 'Trip', userId: user.id });
		const chat = createConversation({
			profile,
			presetId: preset.id,
			userId: user.id,
			folderId: found.id
		});
		const { trashedTo } = deleteFolder(profile, found);
		expect(trashedTo?.startsWith(paths.trash)).toBe(true);
		expect(existsSync(folderDir(profile.slug, found.slug))).toBe(false);
		expect(getConversation(chat.id)?.folderId).toBeNull();
		expect(listFolders(profile.id)).toEqual([]);
	});
});

describe('chats in folders', () => {
	it('start with the folder in their system prompt', async () => {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const found = createFolder({
			profile,
			name: 'Trip',
			userId: user.id,
			instructions: 'We travel with two kids.'
		});
		const map = await upload(profile.id, user.id, 'map.txt', 'north');
		await addFolderFiles({ profile, folder: found, userId: user.id, uploadIds: [map.id] });
		const chat = createConversation({
			profile,
			presetId: preset.id,
			userId: user.id,
			folderId: found.id
		});
		const plain = createConversation({ profile, presetId: preset.id, userId: user.id });

		expect(chat.systemPrompt).toContain('in the folder "Trip"');
		expect(chat.systemPrompt).toContain('We travel with two kids.');
		expect(chat.systemPrompt).toContain(join(folderDir(profile.slug, found.slug), 'map.txt'));
		expect(chat.systemPrompt.endsWith(chat.folderContext)).toBe(true);
		// Everything before the folder is the same as in any other chat of the profile.
		expect(chat.systemPrompt.startsWith(plain.systemPrompt)).toBe(true);
		expect(plain.folderContext).toBe('');
		expect(listConversations(profile.id).find((c) => c.id === chat.id)?.folderId).toBe(found.id);
	});

	it('need a folder of their own profile', () => {
		const { user, profile } = makeFamily();
		const other = makeFamily('Ben');
		const preset = makePreset();
		const theirs = createFolder({ profile: other.profile, name: 'X', userId: other.user.id });
		expect(() =>
			createConversation({ profile, presetId: preset.id, userId: user.id, folderId: theirs.id })
		).toThrow('Unknown folder');
	});

	it('get a new system prompt at the next turn after moving', () => {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const found = createFolder({ profile, name: 'Trip', userId: user.id });
		const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
		say(chat.id, user, 'Hi');
		const first = reply(chat.id, [
			{ type: 'thinking', thinking: 'Greeting.', signature: 'sig-1' },
			{ type: 'text', text: 'Hello!' }
		]);

		moveConversation(profile.id, chat.id, found.id);
		say(chat.id, user, 'Plan the trip');
		const moved = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(moved.systemPrompt).toContain('in the folder "Trip"');
		expect(moved.promptChangedAtSeq).toBe(first.seq);
		expect(getConversation(chat.id)).toEqual(moved);
		// Up to date now: nothing changes on the next call.
		expect(withCurrentContext(moved, committedRows(chat.id))).toBe(moved);

		// The old reply goes without the thinking made under the old prompt.
		const second = reply(chat.id, [
			{ type: 'thinking', thinking: 'Trip.', signature: 'sig-2' },
			{ type: 'text', text: 'Sure.' }
		]);
		say(chat.id, user, 'Thanks');
		expect(
			toAnthropicMessages(requestMessages(committedRows(chat.id), moved.promptChangedAtSeq))
		).toEqual([
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Hi' }] },
			{ role: 'assistant', content: [{ type: 'text', text: 'Hello!' }] },
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Plan the trip' }] },
			{ role: 'assistant', content: JSON.parse(second.content) },
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Thanks' }] }
		]);

		// Moving back out takes the folder out of the prompt again.
		moveConversation(profile.id, chat.id, null);
		const out = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(out.folderContext).toBe('');
		expect(out.systemPrompt).not.toContain('# Folder');
		expect(out.promptChangedAtSeq).toBe(second.seq);
	});

	it('pick up changes to their folder, but not in the middle of a turn', () => {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const found = createFolder({ profile, name: 'Trip', userId: user.id });
		const chat = createConversation({
			profile,
			presetId: preset.id,
			userId: user.id,
			folderId: found.id
		});
		say(chat.id, user, 'Book it');
		reply(chat.id, [{ type: 'tool_use', id: 't1', name: 'run_command', input: { command: 'ls' } }]);
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([{ type: 'tool_result', tool_use_id: 't1', content: 'ok' }])
		});

		setFolderInstructions(found.id, 'Only direct flights.');
		const during = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(during.systemPrompt).not.toContain('Only direct flights.');

		const done = reply(chat.id, [{ type: 'text', text: 'Booked.' }]);
		say(chat.id, user, 'And the hotel?');
		const after = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(after.systemPrompt).toContain('Only direct flights.');
		expect(after.promptChangedAtSeq).toBe(done.seq);
	});

	it('leave out old replies that were only thinking', () => {
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const chat = createConversation({ profile, presetId: preset.id, userId: user.id });
		say(chat.id, user, 'Hi');
		const cut = reply(chat.id, [{ type: 'redacted_thinking', data: 'x' }]);
		say(chat.id, user, 'Hello?');
		expect(toAnthropicMessages(requestMessages(committedRows(chat.id), cut.seq))).toEqual([
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Hi' }] },
			{ role: 'user', content: [{ type: 'text', text: 'Anna: Hello?' }] }
		]);
		// Without a rebuilt prompt, every row goes exactly as stored.
		expect(toAnthropicMessages(requestMessages(committedRows(chat.id), null))[1]).toEqual({
			role: 'assistant',
			content: [{ type: 'redacted_thinking', data: 'x' }]
		});
	});
});
