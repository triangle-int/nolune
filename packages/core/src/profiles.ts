import { randomUUID } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { existsSync, mkdirSync, renameSync } from 'node:fs';
import { join } from 'node:path';
import { and, eq } from 'drizzle-orm';
import { AVATARS, defaultAvatar, isAvatar, type Avatar } from './avatars.ts';
import { getDb } from './db/index.ts';
import { profile, profileMember, user } from './db/schema.ts';
import { paths, profileDir, profileSkillsDir } from './paths.ts';
import { slugBase } from './slugs.ts';
import { findUser } from './users.ts';

export type Profile = typeof profile.$inferSelect;

const holder = globalThis as unknown as {
	__noluneProfileEvents?: EventEmitter;
	__noluneProfileLooks?: Map<string, string>;
};
const emitter = (holder.__noluneProfileEvents ??= new EventEmitter().setMaxListeners(0));
/** Each profile's name, avatar and members as last seen, to tell which changed. */
const looks = (holder.__noluneProfileLooks ??= new Map());

/**
 * Called with the profile's id when its name or avatar changes, or who is in it, or a member's name
 * or picture.
 */
export function onProfileChanged(listener: (profileId: string) => void): () => void {
	emitter.on('changed', listener);
	return () => emitter.off('changed', listener);
}

/**
 * Tells listeners about profiles whose look (name, avatar, members' names and pictures) changed
 * since the last look. The CLI changes them from other processes (the agent runs `nolune profile
 * avatar`), so the gateway also looks after every command and on its scheduler tick. A profile seen
 * for the first time is only noted.
 */
export function noticeProfileChanges(): void {
	const rows = getDb()
		.select({ id: profile.id, name: profile.name, avatar: profile.avatar })
		.from(profile)
		.all();
	const members = new Map<string, string[]>();
	const memberRows = getDb()
		.select({
			profileId: profileMember.profileId,
			id: user.id,
			name: user.name,
			picture: user.picture
		})
		.from(profileMember)
		.innerJoin(user, eq(user.id, profileMember.userId))
		.orderBy(user.id)
		.all();
	for (const row of memberRows) {
		const list = members.get(row.profileId) ?? [];
		list.push(`${row.id} ${row.picture ?? ''} ${row.name}`);
		members.set(row.profileId, list);
	}
	const ids = new Set(rows.map((row) => row.id));
	for (const id of looks.keys()) if (!ids.has(id)) looks.delete(id);
	for (const row of rows) {
		const look = [row.avatar, row.name, ...(members.get(row.id) ?? [])].join('\n');
		if (looks.get(row.id) === look) continue;
		const known = looks.has(row.id);
		looks.set(row.id, look);
		if (known) emitter.emit('changed', row.id);
	}
}

function slugify(name: string): string {
	const base = slugBase(name) || 'profile';
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
	const slug = slugify(trimmed);
	const created: Profile = {
		id: randomUUID(),
		slug,
		name: trimmed,
		avatar: defaultAvatar(slug),
		disabledSkills: [],
		learnFromChats: true,
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
		.select({ id: user.id, name: user.name, picture: user.picture })
		.from(profileMember)
		.innerJoin(user, eq(user.id, profileMember.userId))
		.where(eq(profileMember.profileId, profileId))
		.orderBy(user.name)
		.all();
}

/** `personNote`: their note in memory, when known (addMemberWithNote). */
export function addMember(
	profileId: string,
	nameOrEmail: string,
	personNote: string | null = null
): void {
	const found = findUser(nameOrEmail.trim());
	if (!found) throw new Error(`No user "${nameOrEmail}"`);
	if (isMember(profileId, found.id)) throw new Error(`${found.name} is already a member`);
	getDb().insert(profileMember).values({ profileId, userId: found.id, personNote }).run();
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
	noticeProfileChanges();
}

/** Any member can change the assistant's avatar. Only the web UI shows it, never the model. */
export function setProfileAvatar(profileId: string, avatar: string): Avatar {
	if (!isAvatar(avatar)) {
		throw new Error(`No avatar called "${avatar}". Pick one of: ${AVATARS.join(', ')}.`);
	}
	getDb().update(profile).set({ avatar }).where(eq(profile.id, profileId)).run();
	noticeProfileChanges();
	return avatar;
}

/**
 * Turns skills on or off for this profile's chats: new ones list what's on, and so does a chat
 * already going once its tools are reloaded (reloadTools in runner.ts).
 */
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

/** Whether nolune saves what's worth remembering from the profile's chats by itself (memory-learning). */
export function setLearnFromChats(profileId: string, on: boolean): void {
	getDb().update(profile).set({ learnFromChats: on }).where(eq(profile.id, profileId)).run();
}

/** Deletes the profile and its conversations; the folder is moved to ~/.nolune/trash. */
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
