import { parseArgs } from 'node:util';
import { AVATARS, getProfileBySlug, listProfiles, setProfileAvatar, type Profile } from '@btw/core';

export const PROFILE_HELP = `Profiles and skills
  btw profile list                           slug, name and avatar of every profile
  btw profile avatar [<name>] [--profile SLUG]
                                             show or change the assistant's avatar in the web chat:
                                             ${AVATARS.join(', ')}`;

function profileFor(flag: string | undefined): Profile {
	const slug = flag || process.env.BTW_PROFILE;
	if (!slug) throw new Error('which profile? Pass --profile <slug>. See `btw profile list`.');
	const found = getProfileBySlug(slug);
	if (!found) throw new Error(`no profile with slug "${slug}". See \`btw profile list\`.`);
	return found;
}

export function profileCommand(action: string | undefined, args: string[]): void {
	if (action === 'list') {
		for (const p of listProfiles()) console.log(`${p.slug}\t${p.name}\t${p.avatar}`);
		return;
	}
	if (action !== 'avatar') throw new Error('usage: btw profile list|avatar');

	const { values, positionals } = parseArgs({
		args,
		allowPositionals: true,
		options: { profile: { type: 'string' } }
	});
	const profile = profileFor(values.profile);
	const [name] = positionals;
	if (!name) {
		console.log(profile.avatar);
		return;
	}
	const avatar = setProfileAvatar(profile.id, name.trim().toLowerCase());
	console.log(
		avatar === profile.avatar
			? `${profile.name}'s avatar is already the ${avatar}.`
			: `${profile.name}'s avatar is now the ${avatar}. Open pages show it in a few seconds.`
	);
}
