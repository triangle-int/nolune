import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import Database from 'better-sqlite3';
import { eq } from 'drizzle-orm';
import { drizzle } from 'drizzle-orm/better-sqlite3';
import { migrate } from 'drizzle-orm/better-sqlite3/migrator';
import { describe, expect, it } from 'vitest';
import { AVATARS, GLYPHS, avatarSvg, defaultAvatar, isAvatar } from './avatars.ts';
import { getDb } from './db/index.ts';
import { profile } from './db/schema.ts';
import { paths } from './paths.ts';
import {
	createProfile,
	getProfile,
	noticeProfileChanges,
	onProfileChanged,
	renameProfile,
	setProfileAvatar
} from './profiles.ts';
import { makeUser } from './test/fixtures.ts';

describe('defaultAvatar', () => {
	it('picks the same avatar for a slug every time', () => {
		expect(defaultAvatar('family')).toBe(defaultAvatar('family'));
		expect(isAvatar(defaultAvatar('timur-and-polina'))).toBe(true);
	});

	it('spreads profiles evenly over the set, even slugs that differ only at the end', () => {
		const counts = new Map<string, number>();
		for (let i = 0; i < 800; i++) {
			const avatar = defaultAvatar(`family-${i}`);
			counts.set(avatar, (counts.get(avatar) ?? 0) + 1);
		}
		expect([...counts.keys()].sort()).toEqual([...AVATARS].sort());
		// 100 each if perfectly even.
		for (const count of counts.values()) expect(count).toBeGreaterThan(75);
	});

	it('is what new profiles start with', () => {
		const anna = makeUser('Anna');
		expect(createProfile('Family', anna.id).avatar).toBe(defaultAvatar('family'));
		expect(createProfile('Family', anna.id).avatar).toBe(defaultAvatar('family-2'));
	});
});

describe('the avatars migration', () => {
	it('gives existing profiles the avatar their slug picks', () => {
		// A database as it was before avatars: every migration up to the one that adds them.
		const journal = JSON.parse(readFileSync(join(paths.migrations, 'meta/_journal.json'), 'utf8'));
		const at = journal.entries.findIndex((e: { tag: string }) => e.tag === '0008_avatars');
		const entries = journal.entries.slice(0, at);
		const before = join(paths.home, 'migrations-before-avatars');
		mkdirSync(join(before, 'meta'), { recursive: true });
		for (const { tag } of entries) {
			copyFileSync(join(paths.migrations, `${tag}.sql`), join(before, `${tag}.sql`));
		}
		writeFileSync(join(before, 'meta/_journal.json'), JSON.stringify({ ...journal, entries }));

		const client = new Database(join(paths.home, 'old.db'));
		const db = drizzle(client);
		migrate(db, { migrationsFolder: before });
		const slugs = ['family', 'grandma', 'work', 'homework', 'timur-and-polina', 'zoe-max-2'];
		const insert = client.prepare('insert into profile (id, slug, name) values (?, ?, ?)');
		for (const slug of slugs) insert.run(`id-${slug}`, slug, slug);

		migrate(db, { migrationsFolder: paths.migrations });
		const rows = client.prepare('select slug, avatar from profile').all() as {
			slug: string;
			avatar: string;
		}[];
		client.close();
		expect(Object.fromEntries(rows.map((r) => [r.slug, r.avatar]))).toEqual(
			Object.fromEntries(slugs.map((slug) => [slug, defaultAvatar(slug)]))
		);
		expect(new Set(rows.map((r) => r.avatar)).size).toBeGreaterThan(3);
	});
});

describe('setProfileAvatar', () => {
	it('changes the avatar and refuses names outside the set', () => {
		const family = createProfile('Family', makeUser('Anna').id);
		expect(setProfileAvatar(family.id, 'comet')).toBe('comet');
		expect(getProfile(family.id)?.avatar).toBe('comet');
		expect(() => setProfileAvatar(family.id, 'rocket')).toThrow(
			'No avatar called "rocket". Pick one of: probe, campfire, lantern, planet, quantum, comet, moon, satellite.'
		);
		expect(getProfile(family.id)?.avatar).toBe('comet');
	});
});

describe('noticeProfileChanges', () => {
	it('reports name and avatar changes, including ones made by another process', () => {
		const family = createProfile('Family', makeUser('Anna').id);
		const changed: string[] = [];
		const stop = onProfileChanged((id) => changed.push(id));
		try {
			noticeProfileChanges();
			expect(changed).toEqual([]);

			setProfileAvatar(family.id, 'moon');
			renameProfile(family.id, 'The Family');
			expect(changed).toEqual([family.id, family.id]);

			// What `nolune profile avatar` does from the CLI: the row changes, nothing is emitted here.
			getDb().update(profile).set({ avatar: 'planet' }).where(eq(profile.id, family.id)).run();
			noticeProfileChanges();
			noticeProfileChanges();
			expect(changed).toHaveLength(3);
		} finally {
			stop();
		}
	});
});

describe('glyphs', () => {
	it('draws every avatar in one color on a 24 grid', () => {
		for (const avatar of AVATARS) {
			const glyph = GLYPHS[avatar];
			expect(glyph.eyes.length, avatar).toBeGreaterThan(0);
			const svg = avatarSvg(avatar, '#123456', 'x');
			expect(svg).toContain('viewBox="0 0 24 24"');
			expect(svg).not.toMatch(/stroke|gradient|NaN/);
			expect(svg.match(/#[0-9a-f]{6}/gi)?.every((c) => c === '#123456')).toBe(true);
		}
	});
});
