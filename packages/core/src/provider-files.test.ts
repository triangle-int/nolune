import { existsSync, rmSync } from 'node:fs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { appendRow, createConversation } from './conversations.ts';
import { getDb } from './db/index.ts';
import { providerFile } from './db/schema.ts';
import type { Block, ImageBlock, Message } from './format.ts';
import { MAX_CONVERSATION_IMAGE_BYTES } from './images.ts';
import { blobPath, pruneMedia, storeBytes } from './media.ts';
import { providerFileId, pruneProviderFiles, resolveFiles } from './provider-files.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

/** A provider's Files API, in memory. */
const fakeFiles = vi.hoisted(() => (prefix: string) => {
	const stored = new Set<string>();
	let count = 0;
	let account = 'account-1';
	return {
		stored,
		useAccount: (name: string) => (account = name),
		account: vi.fn(() => account),
		upload: vi.fn(async () => {
			const id = `${prefix}${++count}`;
			stored.add(id);
			return id;
		}),
		exists: vi.fn(async (id: string) => stored.has(id)),
		remove: vi.fn(async (id: string) => {
			stored.delete(id);
		})
	};
});
const files = vi.hoisted(() => fakeFiles('file_'));
const openaiFiles = vi.hoisted(() => fakeFiles('file-'));

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	anthropicFiles: files
}));

vi.mock('./openai-chat.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./openai-chat.ts')>()),
	openaiFiles
}));

const HOUR = 60 * 60 * 1000;
const photo = Buffer.from('a photo');

const upload = (data: Buffer = photo) =>
	providerFileId('anthropic', data, 'photo.jpg', 'image/jpeg');
const cached = () => getDb().select().from(providerFile).all();

beforeEach(() => {
	files.stored.clear();
	files.useAccount('account-1');
	openaiFiles.stored.clear();
	vi.clearAllMocks();
});

/** A picture kept in the media store, as btw's format refers to it. */
function keptPicture(data: Buffer = photo): ImageBlock {
	const { sha256, bytes } = storeBytes(data);
	return { type: 'image', source: { type: 'media', sha256, mime: 'image/jpeg', bytes } };
}

const said = (...blocks: Block[]): Message[] => [{ role: 'user', blocks }];

afterEach(() => {
	vi.useRealTimers();
	vi.restoreAllMocks();
});

describe('providerFileId', () => {
	it('uploads each content once per account', async () => {
		const id = await upload();
		expect(await upload()).toBe(id);
		expect(files.upload).toHaveBeenCalledTimes(1);
		expect(cached()).toMatchObject([{ provider: 'anthropic', account: 'account-1', fileId: id }]);

		files.useAccount('account-2');
		expect(await upload()).not.toBe(id);
		expect(files.upload).toHaveBeenCalledTimes(2);
	});

	it('uploads again when the provider lost the file', async () => {
		const id = await upload();
		files.stored.delete(id);
		const again = await upload();
		expect(again).not.toBe(id);
		expect(cached().map((row) => row.fileId)).toEqual([again]);
	});

	it('uploads content sent twice at once only once', async () => {
		const [a, b] = await Promise.all([upload(), upload()]);
		expect(a).toBe(b);
		expect(files.upload).toHaveBeenCalledTimes(1);
	});
});

describe('resolveFiles', () => {
	it('gives each provider its own copy, uploaded once, and the same one every time', async () => {
		const picture = keptPicture();
		const messages = said(picture);
		const claude = await resolveFiles(messages, 'anthropic');
		const [claudeId] = files.stored;
		expect(claude[0].blocks).toEqual([
			{ type: 'image', source: { type: 'uploaded', provider: 'anthropic', fileId: claudeId } }
		]);
		expect(await resolveFiles(messages, 'anthropic')).toEqual(claude);
		expect(files.upload).toHaveBeenCalledTimes(1);
		// A known copy isn't checked on every request.
		expect(files.exists).not.toHaveBeenCalled();

		const gpt = await resolveFiles(messages, 'openai');
		const [gptId] = openaiFiles.stored;
		expect(gptId).toMatch(/^file-/);
		expect(gpt[0].blocks).toEqual([
			{ type: 'image', source: { type: 'uploaded', provider: 'openai', fileId: gptId } }
		]);
		// The stored messages are left as they were; one without kept files comes back as it is.
		expect(messages[0].blocks).toEqual([picture]);
		const text = said({ type: 'text', text: 'Hi' });
		expect(await resolveFiles(text, 'anthropic')).toBe(text);
	});

	it('puts them inline for the Claude plan, up to what a conversation takes inline', async () => {
		const picture = keptPicture();
		const result: Block = { type: 'tool_result', callId: 't1', content: [picture], isError: false };
		const inline = { type: 'inline', mime: 'image/jpeg', data: photo.toString('base64') };
		expect((await resolveFiles(said(picture, result), 'claude-plan'))[0].blocks).toEqual([
			{ type: 'image', source: inline },
			{ ...result, content: [{ type: 'image', source: inline }] }
		]);

		const nearlyFull: ImageBlock = {
			type: 'image',
			source: {
				type: 'inline',
				mime: 'image/png',
				data: 'x'.repeat(MAX_CONVERSATION_IMAGE_BYTES - 10)
			}
		};
		expect((await resolveFiles(said(nearlyFull, picture), 'claude-plan'))[0].blocks).toEqual([
			nearlyFull,
			{
				type: 'text',
				text: expect.stringMatching(/^\[Picture not shown: this chat already holds as many/)
			}
		]);
	});

	it("fails the call when a provider can't get its copy", async () => {
		files.upload.mockRejectedValueOnce(new Error('Service unavailable'));
		await expect(resolveFiles(said(keptPicture()), 'anthropic')).rejects.toThrow(
			"Couldn't give Anthropic a picture from this chat: Service unavailable"
		);
		expect(cached()).toEqual([]);

		const gone = keptPicture(Buffer.from('deleted'));
		if (gone.source.type === 'media') rmSync(blobPath(gone.source.sha256));
		await expect(resolveFiles(said(gone), 'claude-plan')).rejects.toThrow(
			'A picture is no longer on this computer.'
		);
	});
});

describe('pruneProviderFiles', () => {
	it('deletes files no message refers to once they are unused for an hour', async () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		const sent = await upload(Buffer.from('sent'));
		const unused = await upload(Buffer.from('unused'));
		const recent = Buffer.from('recent');
		const recentId = await upload(recent);
		files.useAccount('account-2');
		const elsewhere = await upload(Buffer.from('other account'));
		files.useAccount('account-1');
		appendRow({
			conversationId: chat.id,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([
				{
					type: 'tool_result',
					tool_use_id: 't1',
					content: [{ type: 'image', source: { type: 'file', file_id: sent } }]
				}
			])
		});

		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.now() + 2 * HOUR);
		await upload(recent);
		await pruneProviderFiles();

		expect(files.remove.mock.calls).toEqual([[unused]]);
		expect(
			cached()
				.map((row) => row.fileId)
				.sort()
		).toEqual([sent, recentId, elsewhere].sort());
	});

	it('keeps copies of what messages keep by reference, and the files themselves', async () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		const picture = keptPicture(Buffer.from('kept'));
		const other = keptPicture(Buffer.from('never sent'));
		const [keptCopy, otherCopy] = (
			await resolveFiles(said(picture, other), 'anthropic')
		)[0].blocks.map((b) => ((b as ImageBlock).source as { fileId: string }).fileId);
		appendRow({ conversationId: chat.id, role: 'user', kind: 'trigger', blocks: [picture] });

		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.now() + 2 * HOUR);
		await pruneProviderFiles();
		pruneMedia();

		expect(files.remove.mock.calls).toEqual([[otherCopy]]);
		expect(cached().map((row) => row.fileId)).toEqual([keptCopy]);
		const blob = (b: ImageBlock) => blobPath((b.source as { sha256: string }).sha256);
		expect(existsSync(blob(picture))).toBe(true);
		expect(existsSync(blob(other))).toBe(false);
	});

	it('keeps the record when deleting fails, to try again later', async () => {
		const id = await upload();
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.now() + 2 * HOUR);
		const log = vi.spyOn(console, 'error').mockImplementation(() => {});
		files.remove.mockRejectedValueOnce(new Error('Service unavailable'));

		await pruneProviderFiles();
		expect(log).toHaveBeenCalledOnce();
		expect(cached().map((row) => row.fileId)).toEqual([id]);
		await pruneProviderFiles();
		expect(cached()).toEqual([]);
	});
});
