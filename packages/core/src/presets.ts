import { randomUUID } from 'node:crypto';
import { eq, or } from 'drizzle-orm';
import { describeApiError, fetchContextWindow } from './anthropic.ts';
import { getDb } from './db/index.ts';
import { modelPreset } from './db/schema.ts';

export type Preset = typeof modelPreset.$inferSelect;
export const PROVIDERS = ['anthropic'] as const;
export type Provider = (typeof PROVIDERS)[number];

export function listPresets(): Preset[] {
	return getDb().select().from(modelPreset).orderBy(modelPreset.createdAt).all();
}

export function getPreset(id: string): Preset | undefined {
	return getDb().select().from(modelPreset).where(eq(modelPreset.id, id)).get();
}

export function effectiveContextWindow(preset: Preset): number | null {
	return preset.contextWindow ?? preset.modelContextWindow;
}

/** Checks the model exists with the provider and records its context window. */
export async function addPreset(input: {
	provider?: Provider;
	model: string;
	name?: string;
	contextWindow?: number | null;
}): Promise<Preset> {
	const provider = input.provider ?? 'anthropic';
	if (!PROVIDERS.includes(provider)) throw new Error(`Unsupported provider "${provider}"`);
	const model = input.model.trim();
	if (!model) throw new Error('Model is required');
	const name = input.name?.trim() || `${model} (${provider})`;
	if (listPresets().some((p) => p.name.toLowerCase() === name.toLowerCase())) {
		throw new Error(`A preset named "${name}" already exists`);
	}
	if (input.contextWindow != null && !(input.contextWindow > 0)) {
		throw new Error('Context window must be a positive number');
	}

	let modelContextWindow: number | null;
	try {
		modelContextWindow = await fetchContextWindow(model);
	} catch (err) {
		throw new Error(`Could not verify model "${model}": ${describeApiError(err)}`, { cause: err });
	}

	const preset = {
		id: randomUUID(),
		name,
		provider,
		model,
		contextWindow: input.contextWindow ?? null,
		modelContextWindow,
		createdAt: new Date()
	};
	getDb().insert(modelPreset).values(preset).run();
	return preset;
}

/** Existing conversations keep their own copy of provider and model, so this only affects new ones. */
export function removePreset(idOrName: string): void {
	const result = getDb()
		.delete(modelPreset)
		.where(or(eq(modelPreset.id, idOrName), eq(modelPreset.name, idOrName)))
		.run();
	if (result.changes === 0) throw new Error(`No preset "${idOrName}"`);
}
