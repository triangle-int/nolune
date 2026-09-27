import { randomUUID } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { getDb } from '../db/index.ts';
import { modelPreset, user } from '../db/schema.ts';
import type { Provider } from '../models.ts';
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
export function makePreset(
	name = 'Sonnet',
	model = 'claude-sonnet-5',
	provider: Provider = 'anthropic'
): Preset {
	const preset: Preset = {
		id: randomUUID(),
		name,
		provider,
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

/** A PDF's catalog and page tree, as plain objects or in a compressed object stream. */
export function pdfWithPages(pages: number, compressed = false): Buffer {
	const kids = Array.from({ length: pages }, (_, i) => `${i + 3} 0 R`).join(' ');
	const tree = `<< /Type /Pages /Kids [${kids}] /Count ${pages} >>`;
	const catalog = '<< /Type /Catalog /Pages 2 0 R >>';
	if (!compressed) {
		return Buffer.from(`%PDF-1.4\n1 0 obj ${catalog} endobj\n2 0 obj ${tree} endobj\n%%EOF\n`);
	}
	const objects = deflateSync(`1 0 2 34 ${catalog} ${tree}`);
	return Buffer.concat([
		Buffer.from(
			`%PDF-1.5\n9 0 obj << /Type /ObjStm /N 2 /First 9 /Filter /FlateDecode /Length ${objects.length} >>\nstream\n`
		),
		objects,
		Buffer.from('\nendstream\nendobj\n%%EOF\n')
	]);
}
