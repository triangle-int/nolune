import { createProfile, defaultAvatar, getProfile } from '@btw/core';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { makeUser } from '../../core/src/test/fixtures.ts';
import { profileCommand } from './profile.ts';

function run(action: string, ...args: string[]): string {
	const log = vi.spyOn(console, 'log').mockImplementation(() => {});
	try {
		profileCommand(action, args);
		return log.mock.calls.map((call) => call.join(' ')).join('\n');
	} finally {
		log.mockRestore();
	}
}

afterEach(() => {
	vi.unstubAllEnvs();
});

describe('btw profile avatar', () => {
	it("shows the profile's avatar, from --profile or the agent's BTW_PROFILE", () => {
		createProfile('Family', makeUser('Anna').id);
		expect(run('avatar', '--profile', 'family')).toBe(defaultAvatar('family'));
		vi.stubEnv('BTW_PROFILE', 'family');
		expect(run('avatar')).toBe(defaultAvatar('family'));
	});

	it('changes it', () => {
		const family = createProfile('Family', makeUser('Anna').id);
		vi.stubEnv('BTW_PROFILE', 'family');
		expect(run('avatar', 'Comet')).toBe(
			"Family's avatar is now the comet. Open pages show it in a few seconds."
		);
		expect(getProfile(family.id)?.avatar).toBe('comet');
		expect(run('avatar', 'comet')).toBe("Family's avatar is already the comet.");
	});

	it('lists the avatars when the name is wrong', () => {
		const family = createProfile('Family', makeUser('Anna').id);
		expect(() => run('avatar', 'rocket', '--profile', 'family')).toThrow(
			'No avatar called "rocket". Pick one of: probe, campfire, lantern, planet, quantum, comet, moon, satellite.'
		);
		expect(getProfile(family.id)?.avatar).toBe(family.avatar);
	});

	it('needs a profile that exists', () => {
		vi.stubEnv('BTW_PROFILE', '');
		expect(() => run('avatar', 'moon')).toThrow('which profile?');
		expect(() => run('avatar', 'moon', '--profile', 'nope')).toThrow('no profile with slug "nope"');
	});
});

describe('btw profile list', () => {
	it('shows each profile with its avatar', () => {
		const anna = makeUser('Anna');
		createProfile('Family', anna.id);
		createProfile('Work', anna.id);
		expect(run('list')).toBe(
			`family\tFamily\t${defaultAvatar('family')}\nwork\tWork\t${defaultAvatar('work')}`
		);
	});
});
