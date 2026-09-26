import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, renameSync } from 'node:fs';
import { join } from 'node:path';
import { and, eq } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { profile, profileMember, user } from './db/schema.ts';
import { paths, profileDir, profileSkillsDir } from './paths.ts';
import { findUser } from './users.ts';

export type Profile = typeof profile.$inferSelect;

function slugify(name: string): string {
	const base =
		name
			.normalize('NFKD')
			.replace(/[̀-ͯ]/g, '')
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 40) || 'profile';
	let slug = base;
	for (
		let i = 2;
		getDb().select().from(profile).where(eq(profile.slug, slug)).get() ||
		existsSync(profileDir(slug));
		i++
	) {
		slug = `${base}-${i}`;
	}
	return slug;
}

export function createProfile(name: string, creatorId: string): Profile {
	const trimmed = name.trim();
	if (!trimmed) throw new Error('Profile name is required');
	const created: Profile = {
		id: randomUUID(),
		slug: slugify(trimmed),
		name: trimmed,
		disabledSkills: [],
		createdBy: creatorId,
		createdAt: new Date()
	};
	mkdirSync(profileSkillsDir(created.slug), { recursive: true });
	getDb().transaction((tx) => {
		tx.insert(profile).values(created).run();
		tx.insert(profileMember).values({ profileId: created.id, userId: creatorId }).run();
	});
	return created;
}

export function listProfiles(): Profile[] {
	return getDb().select().from(profile).orderBy(profile.name).all();
}

export function listProfilesForUser(userId: string): Profile[] {
	return getDb()
		.select({ profile })
		.from(profileMember)
		.innerJoin(profile, eq(profile.id, profileMember.profileId))
		.where(eq(profileMember.userId, userId))
		.orderBy(profile.name)
		.all()
		.map((row) => row.profile);
}

export function getProfile(id: string): Profile | undefined {
	return getDb().select().from(profile).where(eq(profile.id, id)).get();
}

export function getProfileBySlug(slug: string): Profile | undefined {
	return getDb().select().from(profile).where(eq(profile.slug, slug)).get();
}

/** The profile, if the user is a member of it. */
export function getProfileForUser(slug: string, userId: string): Profile | undefined {
	return getDb()
		.select({ profile })
		.from(profile)
		.innerJoin(profileMember, eq(profileMember.profileId, profile.id))
		.where(and(eq(profile.slug, slug), eq(profileMember.userId, userId)))
		.get()?.profile;
}

export function isMember(profileId: string, userId: string): boolean {
	return !!getDb()
		.select()
		.from(profileMember)
		.where(and(eq(profileMember.profileId, profileId), eq(profileMember.userId, userId)))
		.get();
}

export function listMembers(profileId: string) {
	return getDb()
		.select({ id: user.id, name: user.name })
		.from(profileMember)
		.innerJoin(user, eq(user.id, profileMember.userId))
		.where(eq(profileMember.profileId, profileId))
		.orderBy(user.name)
		.all();
}

export function addMember(profileId: string, nameOrEmail: string): void {
	const found = findUser(nameOrEmail.trim());
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	if (isMember(profileId, found.id)) throw new Error(`${found.name} is already a member`);
	getDb().insert(profileMember).values({ profileId, userId: found.id }).run();
}

export function removeMember(profileId: string, userId: string): void {
	getDb()
		.delete(profileMember)
		.where(and(eq(profileMember.profileId, profileId), eq(profileMember.userId, userId)))
		.run();
}

/** Only the display name changes; the folder slug stays so skill paths in prompts stay valid. */
export function renameProfile(profileId: string, name: string): void {
	const trimmed = name.trim();
	if (!trimmed) throw new Error('Profile name is required');
	getDb().update(profile).set({ name: trimmed }).where(eq(profile.id, profileId)).run();
}

/** Turns skills on or off for this profile's new chats. Chats already started keep their prompt. */
export function setSkillsEnabled(profileId: string, names: string[], enabled: boolean): void {
	getDb().transaction((tx) => {
		const found = tx.select().from(profile).where(eq(profile.id, profileId)).get();
		if (!found) throw new Error('No such profile');
		const disabled = new Set(found.disabledSkills);
		for (const name of names) {
			if (enabled) disabled.delete(name);
			else disabled.add(name);
		}
		tx.update(profile)
			.set({ disabledSkills: [...disabled].sort() })
			.where(eq(profile.id, profileId))
			.run();
	});
}

/** Deletes the profile and its conversations; the folder is moved to ~/.btw-agent/trash. */
export function deleteProfile(profileId: string): { trashedTo: string | null } {
	const found = getDb().select().from(profile).where(eq(profile.id, profileId)).get();
	if (!found) throw new Error('No such profile');
	let trashedTo: string | null = null;
	const dir = profileDir(found.slug);
	if (existsSync(dir)) {
		mkdirSync(paths.trash, { recursive: true });
		trashedTo = join(paths.trash, `${found.slug}-${Date.now()}`);
		renameSync(dir, trashedTo);
	}
	getDb().delete(profile).where(eq(profile.id, profileId)).run();
	return { trashedTo };
}
