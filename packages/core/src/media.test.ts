import { existsSync, utimesSync } from 'node:fs';
import { Readable } from 'node:stream';
import { describe, expect, it } from 'vitest';
import { createUpload } from './attachments.ts';
import { appendRow, createConversation, deleteConversation } from './conversations.ts';
import {
	blobPath,
	getMedia,
	listMedia,
	mediaByMessage,
	pruneMedia,
	store,
	toDisplayMedia,
	type PreparedMedia
} from './media.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';
import { clearUserPicture, setUserPicture } from './users.ts';

const PNG = Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex');
/** The start of a 256-pixel square PNG: all inspectImage reads. */
const PNG_256 = Buffer.concat([PNG, Buffer.from('000001000000010008060000001f15c4', 'hex')]);

function prepared(
	src: string,
	sha256: string | null,
	extra: Partial<PreparedMedia> = {}
): PreparedMedia {
	return {
		src,
		status: 'ok',
		error: null,
		name: src.split('/').at(-1)!,
		sha256,
		mime: 'image/png',
		bytes: 16,
		width: 1,
		height: 1,
		previewSha256: null,
		...extra
	};
}

async function blob(data: string | Buffer, ageMs = 2 * 60 * 60 * 1000): Promise<string> {
	const { sha256 } = await store(Readable.from([Buffer.from(data)]));
	const then = new Date(Date.now() - ageMs);
	utimesSync(blobPath(sha256), then, then);
	return sha256;
}

function newChat() {
	const { user, profile } = makeFamily();
	return {
		user,
		profile,
		chat: createConversation({ profile, presetId: makePreset().id, userId: user.id })
	};
}

describe('media rows', () => {
	it('groups copies by reply and finds one only in its own conversation', async () => {
		const { user, profile, chat } = newChat();
		const other = createConversation({ profile, presetId: chat.presetId!, userId: user.id });
		const sha = await blob(PNG);
		const reply = appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: '[]',
			media: [
				prepared('/Users/anna/beach.png', sha),
				prepared('/Users/anna/gone.png', null, {
					status: 'missing',
					error: 'Not found',
					mime: null,
					bytes: null
				})
			]
		});

		const rows = listMedia(reply.id);
		expect(mediaByMessage(chat.id)).toEqual(new Map([[reply.id, rows]]));
		expect(getMedia(chat.id, rows[0].id)?.src).toBe('/Users/anna/beach.png');
		expect(getMedia(other.id, rows[0].id)).toBeUndefined();
		expect(toDisplayMedia(rows)).toEqual({
			'/Users/anna/beach.png': {
				status: 'ok',
				id: rows[0].id,
				name: 'beach.png',
				mime: 'image/png',
				bytes: 16,
				width: 1,
				height: 1,
				viewable: true
			},
			'/Users/anna/gone.png': { status: 'missing', name: 'gone.png', error: 'Not found' }
		});
	});
});

describe('pruneMedia', () => {
	it('deletes stored files nothing refers to, once they are an hour old', async () => {
		const { user, profile, chat } = newChat();
		const shown = await blob('shown');
		const preview = await blob('preview');
		const attached = await createUpload({
			profileId: profile.id,
			userId: user.id,
			name: 'a.txt',
			body: Readable.from([Buffer.from('attached')])
		});
		const orphan = await blob('orphan');
		const fresh = await blob('fresh', 60_000);
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: '[]',
			media: [prepared('photo.heic', shown, { mime: 'image/heic', previewSha256: preview })]
		});

		pruneMedia();
		expect(existsSync(blobPath(orphan))).toBe(false);
		for (const kept of [shown, preview, attached.sha256, fresh]) {
			expect(existsSync(blobPath(kept))).toBe(true);
		}

		deleteConversation(chat.id);
		pruneMedia();
		expect(existsSync(blobPath(shown))).toBe(false);
		expect(existsSync(blobPath(preview))).toBe(false);
	});

	it("keeps people's profile pictures until they change them", async () => {
		const { user } = makeFamily();
		const picture = setUserPicture(user.id, PNG_256);
		const then = new Date(Date.now() - 2 * 60 * 60 * 1000);
		utimesSync(blobPath(picture), then, then);

		pruneMedia();
		expect(existsSync(blobPath(picture))).toBe(true);

		clearUserPicture(user.id);
		pruneMedia();
		expect(existsSync(blobPath(picture))).toBe(false);
	});
});
