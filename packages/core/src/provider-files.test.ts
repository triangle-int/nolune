import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { appendRow, createConversation } from './conversations.ts';
import { getDb } from './db/index.ts';
import { providerFile } from './db/schema.ts';
import { providerFileId, pruneProviderFiles } from './provider-files.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

/** Anthropic's Files API, in memory. */
const files = vi.hoisted(() => {
	const stored = new Set<string>();
	let count = 0;
	let account = 'account-1';
	return {
		stored,
		useAccount: (name: string) => (account = name),
		account: vi.fn(() => account),
		upload: vi.fn(async () => {
			const id = `file_${++count}`;
			stored.add(id);
			return id;
		}),
		exists: vi.fn(async (id: string) => stored.has(id)),
		remove: vi.fn(async (id: string) => {
			stored.delete(id);
		})
	};
});

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	anthropicFiles: files
}));

const HOUR = 60 * 60 * 1000;
const photo = Buffer.from('a photo');

const upload = (data: Buffer = photo) =>
	providerFileId('anthropic', data, 'photo.jpg', 'image/jpeg');
const cached = () => getDb().select().from(providerFile).all();

beforeEach(() => {
	files.stored.clear();
	files.useAccount('account-1');
	vi.clearAllMocks();
});

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
