import { MAX_SOUL_CHARS, readSoulFile, writeSoul } from '@btw/core';
import type { Io } from './io.ts';
import { splitProfile } from './memory.ts';
import { profileFor } from './profile.ts';

export const SOUL_HELP = `Soul (who btw is for a profile: character, values, tone; at most ${MAX_SOUL_CHARS} characters)
  btw soul [show] [--profile SLUG]           print it
  btw soul write [text]                      replace it (the text, or stdin); every chat gets it
                                             from its next message
  btw soul rm                                remove it`;

export async function soulCommand(io: Io, args: string[]): Promise<void> {
	const { profile: flag, words } = splitProfile(args);
	const [action = 'show', ...rest] = words;
	if (action === 'help') {
		io.log(SOUL_HELP);
		return;
	}
	const profile = profileFor(io, flag);

	switch (action) {
		case 'show': {
			const text = readSoulFile(profile.slug);
			if (!text) {
				io.log(`${profile.name} has no soul yet. Start one with: btw soul write < soul.md`);
				return;
			}
			io.log(text);
			return;
		}
		case 'write': {
			if (!rest.length && io.stdinIsTTY) {
				throw new Error('give the soul as text, or pipe it in: btw soul write < soul.md');
			}
			const text = rest.length ? rest.join(' ') : await io.readStdin();
			if (!text.trim()) throw new Error('the soul is empty. To remove it, use `btw soul rm`.');
			writeSoul(profile.slug, text);
			io.log(`Saved the soul of ${profile.name}. Every chat gets it from its next message.`);
			return;
		}
		case 'rm': {
			writeSoul(profile.slug, '');
			io.log(`Removed the soul of ${profile.name}.`);
			return;
		}
		default:
			throw new Error(`unknown soul command "${action}". See \`btw soul help\`.`);
	}
}
