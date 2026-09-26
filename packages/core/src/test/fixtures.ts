import { randomUUID } from 'node:crypto';
import { getDb } from '../db/index.ts';
import { modelPreset, user } from '../db/schema.ts';
import type { Preset } from '../presets.ts';
import { createProfile, type Profile } from '../profiles.ts';

/** A user without a password account, which createUser would hash slowly. */
export function makeUser(name = 'Anna'): { id: string; name: string } {
	const id = randomUUID();
	getDb()
		.insert(user)
		.values({ id, name, email: `${name.toLowerCase()}@example.com` })
		.run();
	return { id, name };
}

let presetClock = Date.UTC(2026, 0, 1);

/** A preset without asking the API whether the model exists, as addPreset does. */
export function makePreset(name = 'Sonnet', model = 'claude-sonnet-5'): Preset {
	const preset: Preset = {
		id: randomUUID(),
		name,
		provider: 'anthropic',
		model,
		contextWindow: null,
		modelContextWindow: 200_000,
		isDefault: false,
		// Apart, so listPresets' order by creation is the order they were made in.
		createdAt: new Date((presetClock += 1000))
	};
	getDb().insert(modelPreset).values(preset).run();
	return preset;
}

/** A user with a profile of their own, the usual starting point. */
export function makeFamily(name = 'Anna'): {
	user: { id: string; name: string };
	profile: Profile;
} {
	const member = makeUser(name);
	return { user: member, profile: createProfile('Family', member.id) };
}
