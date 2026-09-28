import { parseArgs } from 'node:util';
import {
	AVATARS,
	getProfileBySlug,
	listProfiles,
	setProfileAvatar,
	type Profile
} from '@nolune/core';
import type { Io } from './io.ts';

export const PROFILE_HELP = `Profiles and skills
  nolune profile list                        slug, name and avatar of every profile
  nolune profile avatar [<name>] [--profile SLUG]
                                             show or change the assistant's avatar in the web chat:
                                             ${AVATARS.join(', ')}`;

/** The profile `--profile` names, else the one the agent's command runs in (NOLUNE_PROFILE). */
export function profileFor(io: Io, flag: string | undefined): Profile {
	const slug = flag || io.env.NOLUNE_PROFILE;
	if (!slug) throw new Error('which profile? Pass --profile <slug>. See `nolune profile list`.');
	const found = getProfileBySlug(slug);
	if (!found) throw new Error(`no profile with slug "${slug}". See \`nolune profile list\`.`);
	return found;
}

export function profileCommand(io: Io, action: string | undefined, args: string[]): void {
	if (action === 'list') {
		for (const p of listProfiles()) io.log(`${p.slug}\t${p.name}\t${p.avatar}`);
		return;
	}
	if (action !== 'avatar') throw new Error('usage: nolune profile list|avatar');

	const { values, positionals } = parseArgs({
		args,
		allowPositionals: true,
		options: { profile: { type: 'string' } }
	});
	const profile = profileFor(io, values.profile);
	const [name] = positionals;
	if (!name) {
		io.log(profile.avatar);
		return;
	}
	const avatar = setProfileAvatar(profile.id, name.trim().toLowerCase());
	io.log(
		avatar === profile.avatar
			? `${profile.name}'s avatar is already the ${avatar}.`
			: `${profile.name}'s avatar is now the ${avatar}. Open pages show it in a few seconds.`
	);
}
