import { randomUUID } from 'node:crypto';
import { eq, or } from 'drizzle-orm';
import { findCustomProvider, isCustomProvider, splitModel } from './custom-providers.ts';
import { getDb } from './db/index.ts';
import { modelPreset } from './db/schema.ts';
import { describeApiError, fetchContextWindow, isProvider, type Provider } from './models.ts';

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

/** By id, or by name as `nolune preset list` shows it. */
export function findPreset(idOrName: string): Preset {
	const preset = getDb()
		.select()
		.from(modelPreset)
		.where(or(eq(modelPreset.id, idOrName), eq(modelPreset.name, idOrName)))
		.get();
	if (!preset) throw new Error(`No preset "${idOrName}"`);
	return preset;
}

/** Existing conversations keep their model; automations without a preset pick this up. */
export function setDefaultPreset(idOrName: string): Preset {
	const preset = findPreset(idOrName);
	getDb().transaction((tx) => {
		tx.update(modelPreset).set({ isDefault: false }).where(eq(modelPreset.isDefault, true)).run();
		tx.update(modelPreset).set({ isDefault: true }).where(eq(modelPreset.id, preset.id)).run();
	});
	return { ...preset, isDefault: true };
}

export function effectiveContextWindow(preset: Preset): number | null {
	return preset.contextWindow ?? preset.modelContextWindow;
}

/** What a preset is called when it isn't given a name: a custom provider's model by its name. */
function defaultName(model: string, provider: string): string {
	if (isCustomProvider(provider)) {
		const on = splitModel(model);
		const custom = on.provider ? findCustomProvider(on.provider) : undefined;
		if (custom) return `${on.model} (${custom.name})`;
	}
	return `${model} (${provider})`;
}

function checkName(name: string, except?: string): void {
	const taken = listPresets().some(
		(p) => p.id !== except && p.name.toLowerCase() === name.toLowerCase()
	);
	if (taken) throw new Error(`A preset named "${name}" already exists`);
}

function checkContextWindow(contextWindow: number | null | undefined): void {
	if (contextWindow != null && !(contextWindow > 0)) {
		throw new Error('Context window must be a positive number');
	}
}

/** The model's own window, from its provider, which also says whether it knows the model. */
async function verifyModel(provider: Provider, model: string): Promise<number | null> {
	try {
		return await fetchContextWindow(provider, model);
	} catch (err) {
		throw new Error(`Could not verify model "${model}": ${describeApiError(err)}`, { cause: err });
	}
}

/**
 * Checks the model exists with the provider and records its context window, when the provider
 * says (OpenAI doesn't: without an override, its presets have none).
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
	const name = input.name?.trim() || defaultName(model, provider);
	checkName(name);
	checkContextWindow(input.contextWindow);
	const modelContextWindow = await verifyModel(provider, model);

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

/**
 * Changes a preset; what's left out stays as it was. New chats, automations that use it, and
 * chats switched to it from now on get the change; chats already on it keep the copy they took
 * (setPreset). A new provider or model is checked like a new preset's, and only then: renaming
 * or setting the window needs no key. A name left as the default follows the model.
 */
export async function editPreset(
	idOrName: string,
	change: {
		/** As typed, like addPreset's. */
		provider?: string;
		model?: string;
		/** Empty for the default name. */
		name?: string;
		/** Null for the model's own window. */
		contextWindow?: number | null;
	}
): Promise<Preset> {
	const preset = findPreset(idOrName);
	const provider = change.provider?.trim() || preset.provider;
	if (!isProvider(provider)) throw new Error(`Unsupported provider "${provider}"`);
	const model = change.model === undefined ? preset.model : change.model.trim();
	if (!model) throw new Error('Model is required');
	const hadDefaultName = preset.name === defaultName(preset.model, preset.provider);
	const name =
		change.name === undefined
			? hadDefaultName
				? defaultName(model, provider)
				: preset.name
			: change.name.trim() || defaultName(model, provider);
	checkName(name, preset.id);
	checkContextWindow(change.contextWindow);
	const contextWindow =
		change.contextWindow === undefined ? preset.contextWindow : change.contextWindow;
	const modelChanged = provider !== preset.provider || model !== preset.model;
	const modelContextWindow = modelChanged
		? await verifyModel(provider, model)
		: preset.modelContextWindow;

	const changed = { name, provider, model, contextWindow, modelContextWindow };
	getDb().update(modelPreset).set(changed).where(eq(modelPreset.id, preset.id)).run();
	return { ...preset, ...changed };
}

/** Existing conversations keep their own copy of provider and model, so this only affects new ones. */
export function removePreset(idOrName: string): void {
	const result = getDb()
		.delete(modelPreset)
		.where(or(eq(modelPreset.id, idOrName), eq(modelPreset.name, idOrName)))
		.run();
	if (result.changes === 0) throw new Error(`No preset "${idOrName}"`);
}
