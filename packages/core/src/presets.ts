import { randomUUID } from 'node:crypto';
import { eq, or } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { modelPreset } from './db/schema.ts';
import { describeApiError, fetchContextWindow, isProvider } from './models.ts';

export type Preset = typeof modelPreset.$inferSelect;

export function listPresets(): Preset[] {
	return getDb().select().from(modelPreset).orderBy(modelPreset.createdAt).all();
}

export function getPreset(id: string): Preset | undefined {
	return getDb().select().from(modelPreset).where(eq(modelPreset.id, id)).get();
}

/** What new chats start with, and what automations use when they don't name a preset. */
export function getDefaultPreset(): Preset | undefined {
	const presets = listPresets();
	return presets.find((p) => p.isDefault) ?? presets[0];
}

/** Existing conversations keep their model; automations without a preset pick this up. */
export function setDefaultPreset(idOrName: string): Preset {
	const db = getDb();
	const preset = db
		.select()
		.from(modelPreset)
		.where(or(eq(modelPreset.id, idOrName), eq(modelPreset.name, idOrName)))
		.get();
	if (!preset) throw new Error(`No preset "${idOrName}"`);
	db.transaction((tx) => {
		tx.update(modelPreset).set({ isDefault: false }).where(eq(modelPreset.isDefault, true)).run();
		tx.update(modelPreset).set({ isDefault: true }).where(eq(modelPreset.id, preset.id)).run();
	});
	return { ...preset, isDefault: true };
}

export function effectiveContextWindow(preset: Preset): number | null {
	return preset.contextWindow ?? preset.modelContextWindow;
}

/**
 * Checks the model exists with the provider and records its context window, when the provider
 * says (OpenAI doesn't: without an override, its presets have none; ChatGPT's Codex catalog does).
 */
export async function addPreset(input: {
	/** As typed (the CLI, the admin page): checked here. Anthropic when left out. */
	provider?: string;
	model: string;
	name?: string;
	contextWindow?: number | null;
}): Promise<Preset> {
	const provider = input.provider?.trim() || 'anthropic';
	if (!isProvider(provider)) throw new Error(`Unsupported provider "${provider}"`);
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
		modelContextWindow = await fetchContextWindow(provider, model);
	} catch (err) {
		throw new Error(`Could not verify model "${model}": ${describeApiError(err, provider)}`, {
			cause: err
		});
	}

	const preset = {
		id: randomUUID(),
		name,
		provider,
		model,
		contextWindow: input.contextWindow ?? null,
		modelContextWindow,
		isDefault: false,
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
