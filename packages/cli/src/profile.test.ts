import { createProfile, defaultAvatar, getProfile } from '@nolune/core';
import { describe, expect, it } from 'vitest';
import { makeUser } from '../../core/src/test/fixtures.ts';
import { profileCommand } from './profile.ts';
import { testIo } from './test/io.ts';

function run(env: Record<string, string>, action: string, ...args: string[]): string {
	const { io, out } = testIo({ env });
	profileCommand(io, action, args);
	return out().trimEnd();
}

describe('nolune profile avatar', () => {
	it("shows the profile's avatar, from --profile or the agent's NOLUNE_PROFILE", () => {
		createProfile('Family', makeUser('Anna').id);
		expect(run({}, 'avatar', '--profile', 'family')).toBe(defaultAvatar('family'));
		expect(run({ NOLUNE_PROFILE: 'family' }, 'avatar')).toBe(defaultAvatar('family'));
	});

	it('changes it', () => {
		const family = createProfile('Family', makeUser('Anna').id);
		const env = { NOLUNE_PROFILE: 'family' };
		expect(run(env, 'avatar', 'Comet')).toBe(
			"Family's avatar is now the comet. Open pages show it in a few seconds."
		);
		expect(getProfile(family.id)?.avatar).toBe('comet');
		expect(run(env, 'avatar', 'comet')).toBe("Family's avatar is already the comet.");
	});

	it('lists the avatars when the name is wrong', () => {
		const family = createProfile('Family', makeUser('Anna').id);
		expect(() => run({}, 'avatar', 'rocket', '--profile', 'family')).toThrow(
			'No avatar called "rocket". Pick one of: probe, campfire, lantern, planet, quantum, comet, moon, satellite.'
		);
		expect(getProfile(family.id)?.avatar).toBe(family.avatar);
	});

	it('needs a profile that exists', () => {
		expect(() => run({}, 'avatar', 'moon')).toThrow('which profile?');
		expect(() => run({}, 'avatar', 'moon', '--profile', 'nope')).toThrow(
			'no profile with slug "nope"'
		);
	});
});

describe('nolune profile list', () => {
	it('shows each profile with its avatar', () => {
		const anna = makeUser('Anna');
		createProfile('Family', anna.id);
		createProfile('Work', anna.id);
		expect(run({}, 'list')).toBe(
			`family\tFamily\t${defaultAvatar('family')}\nwork\tWork\t${defaultAvatar('work')}`
		);
	});
});
