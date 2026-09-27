import { existsSync, readdirSync } from 'node:fs';
import { Readable } from 'node:stream';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	cleanFileName,
	createUpload,
	deleteUpload,
	findUploads,
	MAX_ATTACHMENTS,
	pdfPageCount,
	pruneUploads
} from './attachments.ts';
import { blobPath } from './media.ts';
import { paths } from './paths.ts';
import { makeFamily, makeUser, pdfWithPages } from './test/fixtures.ts';

afterEach(() => {
	vi.useRealTimers();
});

const PNG = Buffer.from('89504e470d0a1a0a0000000d49484452', 'hex');

function attach(profileId: string, userId: string, name: string, data: Buffer | string = 'hello') {
	return createUpload({ profileId, userId, name, body: Readable.from([Buffer.from(data)]) });
}

it.each([
	['../../etc/passwd', 'passwd'],
	['C:\\Users\\anna\\report.pdf', 'report.pdf'],
	['.hidden', 'hidden'],
	['bad\x00name\n.txt', 'badname.txt'],
	['...', 'file'],
	[`${'a'.repeat(200)}.jpeg`, `${'a'.repeat(115)}.jpeg`]
])('cleans the file name %j to %j', (name, clean) => {
	expect(cleanFileName(name)).toBe(clean);
});

describe('pdfPageCount', () => {
	it("reads the page tree's count, also from compressed object streams", () => {
		expect(pdfPageCount(pdfWithPages(3))).toBe(3);
		expect(pdfPageCount(pdfWithPages(12, true))).toBe(12);
		expect(pdfPageCount(Buffer.from('%PDF-1.4\n%%EOF\n'))).toBeNull();
	});
});

describe('createUpload', () => {
	it('stores the file once per content, typed by what is in it', async () => {
		const { user, profile } = makeFamily();
		const notes = await attach(profile.id, user.id, '../notes.txt', 'hello');
		const again = await attach(profile.id, user.id, 'copy.txt', 'hello');
		const picture = await attach(profile.id, user.id, 'looks-like.txt', PNG);
		const fake = await attach(profile.id, user.id, 'photo.png', 'not a picture');

		expect(notes).toMatchObject({ name: 'notes.txt', mime: 'text/plain', bytes: 5 });
		expect(again.sha256).toBe(notes.sha256);
		expect(picture.mime).toBe('image/png');
		expect(fake.mime).toBe('application/octet-stream');
		expect(existsSync(blobPath(notes.sha256))).toBe(true);
		expect(readdirSync(paths.media).sort()).toEqual(
			[notes.sha256, picture.sha256, fake.sha256].sort()
		);
	});
});

describe('finding uploads', () => {
	it("returns the sender's uploads in the order asked for", async () => {
		const { user, profile } = makeFamily();
		const a = await attach(profile.id, user.id, 'a.txt', 'a');
		const b = await attach(profile.id, user.id, 'b.txt', 'b');
		expect(findUploads(profile.id, user.id, [b.id, a.id]).map((u) => u.name)).toEqual([
			'b.txt',
			'a.txt'
		]);
		expect(findUploads(profile.id, user.id, [])).toEqual([]);
	});

	it("refuses someone else's upload, the same one twice and too many", async () => {
		const { user, profile } = makeFamily();
		const max = makeUser('Max');
		const mine = await attach(profile.id, user.id, 'a.txt');
		expect(() => findUploads(profile.id, max.id, [mine.id])).toThrow('no longer there');
		expect(() => findUploads(profile.id, user.id, [mine.id, mine.id])).toThrow(
			'A file is attached twice.'
		);
		const ids = Array.from({ length: MAX_ATTACHMENTS + 1 }, (_, i) => `id-${i}`);
		expect(() => findUploads(profile.id, user.id, ids)).toThrow('At most 10 files per message.');
	});

	it('lets only the person who attached a file remove it', async () => {
		const { user, profile } = makeFamily();
		const max = makeUser('Max');
		const upload = await attach(profile.id, user.id, 'a.txt');
		expect(deleteUpload(profile.id, max.id, upload.id)).toBe(false);
		expect(deleteUpload(profile.id, user.id, upload.id)).toBe(true);
		expect(deleteUpload(profile.id, user.id, upload.id)).toBe(false);
	});

	it('forgets uploads nobody sent within a day', async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 28, 7));
		const { user, profile } = makeFamily();
		const old = await attach(profile.id, user.id, 'old.txt', 'old');
		vi.setSystemTime(Date.UTC(2026, 8, 29, 6));
		const recent = await attach(profile.id, user.id, 'recent.txt', 'recent');
		vi.setSystemTime(Date.UTC(2026, 8, 29, 8));

		pruneUploads();
		expect(() => findUploads(profile.id, user.id, [old.id])).toThrow('no longer there');
		expect(findUploads(profile.id, user.id, [recent.id])).toHaveLength(1);
	});
});
