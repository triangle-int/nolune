import { existsSync, mkdirSync, readdirSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { createConversation, getConversation } from './conversations.ts';
import { paths, profileDir, profileSkillsDir } from './paths.ts';
import {
	addMember,
	createProfile,
	deleteProfile,
	getProfile,
	getProfileBySlug,
	getProfileForUser,
	isMember,
	listMembers,
	listProfilesForUser,
	removeMember,
	renameProfile,
	setSkillsEnabled
} from './profiles.ts';
import { makePreset, makeUser } from './test/fixtures.ts';

describe('createProfile', () => {
	it('makes the creator a member and gives the profile a folder', () => {
		const anna = makeUser('Anna');
		const created = createProfile(' Family ', anna.id);
		expect(created).toMatchObject({ slug: 'family', name: 'Family', disabledSkills: [] });
		expect(getProfile(created.id)).toEqual(created);
		expect(isMember(created.id, anna.id)).toBe(true);
		expect(existsSync(profileSkillsDir('family'))).toBe(true);
	});

	it('picks a slug no other profile or folder has', () => {
		const anna = makeUser('Anna');
		expect(createProfile('Zoë & Max!', anna.id).slug).toBe('zoe-max');
		expect(createProfile('Zoe Max', anna.id).slug).toBe('zoe-max-2');
		mkdirSync(profileDir('garden'), { recursive: true });
		expect(createProfile('Garden', anna.id).slug).toBe('garden-2');
		expect(createProfile('🏡', anna.id).slug).toBe('profile');
		expect(() => createProfile('  ', anna.id)).toThrow('Profile name is required');
	});
});

describe('members', () => {
	it('shows people only the profiles they are in', () => {
		const anna = makeUser('Anna');
		const max = makeUser('Max');
		const family = createProfile('Family', anna.id);
		const work = createProfile('Work', max.id);
		addMember(family.id, 'max@example.com');

		expect(listProfilesForUser(anna.id).map((p) => p.name)).toEqual(['Family']);
		expect(listProfilesForUser(max.id).map((p) => p.name)).toEqual(['Family', 'Work']);
		expect(getProfileForUser('work', anna.id)).toBeUndefined();
		expect(getProfileForUser('work', max.id)?.id).toBe(work.id);
		expect(listMembers(family.id).map((m) => m.name)).toEqual(['Anna', 'Max']);

		removeMember(family.id, max.id);
		expect(getProfileForUser('family', max.id)).toBeUndefined();
	});

	it('refuses unknown people and people already in', () => {
		const anna = makeUser('Anna');
		const family = createProfile('Family', anna.id);
		expect(() => addMember(family.id, 'Max')).toThrow('No user "Max"');
		expect(() => addMember(family.id, 'anna')).toThrow('Anna is already a member');
	});
});

describe('changing profiles', () => {
	it('renames without moving the folder', () => {
		const family = createProfile('Family', makeUser().id);
		renameProfile(family.id, ' The Smiths ');
		expect(getProfileBySlug('family')?.name).toBe('The Smiths');
	});

	it('keeps a sorted list of the skills that are off', () => {
		const family = createProfile('Family', makeUser().id);
		setSkillsEnabled(family.id, ['weather', 'automations'], false);
		setSkillsEnabled(family.id, ['weather'], false);
		expect(getProfile(family.id)?.disabledSkills).toEqual(['automations', 'weather']);
		setSkillsEnabled(family.id, ['automations', 'unknown'], true);
		expect(getProfile(family.id)?.disabledSkills).toEqual(['weather']);
		expect(() => setSkillsEnabled('nope', ['weather'], false)).toThrow('No such profile');
	});

	it('deletes the conversations and moves the folder to the trash', () => {
		const anna = makeUser();
		const family = createProfile('Family', anna.id);
		const chat = createConversation({
			profile: family,
			presetId: makePreset().id,
			userId: anna.id
		});

		const { trashedTo } = deleteProfile(family.id);
		expect(getProfile(family.id)).toBeUndefined();
		expect(getConversation(chat.id)).toBeUndefined();
		expect(existsSync(profileDir('family'))).toBe(false);
		expect(trashedTo).toMatch(/family-\d+$/);
		expect(readdirSync(paths.trash)).toHaveLength(1);
		expect(() => deleteProfile(family.id)).toThrow('No such profile');
	});
});
