/**
 * Model presets (#156): the user names the models Nolune may call. Two slots
 * say which preset handles conversations and which does background work, and
 * a chat can pin its own. This module is pure so its rules are testable.
 *
 * @typedef {{ id: string, label: string }} Provider
 * @typedef {{ id: string, name: string, provider: string, model: string }} ModelPreset
 * @typedef {{ chat_preset: string, background_preset: string }} Slots
 */

/** @type {readonly Provider[]} */
export const PROVIDERS = Object.freeze([
	{ id: "anthropic", label: "Anthropic" },
	{ id: "openai", label: "OpenAI" },
	{ id: "openrouter", label: "OpenRouter" },
]);

/**
 * OpenRouter names models `vendor/model`, optionally `vendor/model:variant`
 * (#26). Mirrors `config::is_openrouter_model_id`.
 * @param {string} model
 */
export function isOpenrouterModelId(model) {
	const slash = model.indexOf("/");
	return slash > 0 && slash < model.length - 1 && !/\s/.test(model);
}

/** @param {string} id */
export function providerLabel(id) {
	return PROVIDERS.find((p) => p.id === id)?.label ?? id;
}

const ID_PATTERN = /^[A-Za-z0-9_-]+$/;

/**
 * Every reason the server would reject this shape, in plain words.
 * Mirrors `LlmConfig::validate_presets` so the form can say no before saving.
 *
 * @param {ModelPreset[]} presets
 * @param {Slots} slots
 * @param {string[]} keyedProviders providers that have an API key
 * @returns {string[]}
 */
export function validatePresets(presets, slots, keyedProviders) {
	const errors = [];
	const seen = new Set();
	for (const preset of presets) {
		const id = (preset.id ?? "").trim();
		if (!ID_PATTERN.test(id)) errors.push(`Preset id "${preset.id}" must use letters, digits, "-" or "_".`);
		else if (seen.has(id)) errors.push(`Preset id "${id}" is used twice.`);
		seen.add(id);
		const label = id || preset.name || "(unnamed)";
		if (!(preset.name ?? "").trim()) errors.push(`Preset "${label}" needs a name.`);
		if (!PROVIDERS.some((p) => p.id === preset.provider)) errors.push(`Preset "${label}" uses unknown provider "${preset.provider}".`);
		const model = (preset.model ?? "").trim();
		if (!model) errors.push(`Preset "${label}" needs a model id.`);
		else if (preset.provider === "openrouter" && !isOpenrouterModelId(model)) errors.push(`Preset "${label}" needs an OpenRouter model id in vendor/model form.`);
	}
	if (presets.length === 0) return errors;
	for (const [slot, label] of [["chat_preset", "Chat"], ["background_preset", "Background"]]) {
		const id = slots[/** @type {keyof Slots} */ (slot)];
		const preset = presets.find((p) => p.id === id);
		if (!preset) {
			errors.push(`${label} points at missing preset "${id}".`);
			continue;
		}
		if (!keyedProviders.includes(preset.provider)) {
			errors.push(`${label} uses ${preset.name}, but no ${providerLabel(preset.provider)} API key is configured.`);
		}
	}
	return errors;
}

/**
 * The preset a conversation runs on: its pin when that still exists, else the
 * Chat slot, else the first preset, else nothing.
 *
 * @param {string | null | undefined} pinned
 * @param {Slots} slots
 * @param {ModelPreset[]} presets
 * @returns {string | null}
 */
export function effectivePresetId(pinned, slots, presets) {
	const exists = (/** @type {string | null | undefined} */ id) => !!id && presets.some((p) => p.id === id);
	if (exists(pinned)) return /** @type {string} */ (pinned);
	if (exists(slots?.chat_preset)) return slots.chat_preset;
	return presets[0]?.id ?? null;
}

/** @param {ModelPreset} preset */
export function presetLabel(preset) {
	return `${preset.name} · ${preset.model}`;
}

/**
 * A short human name for a raw model id, for the badge under a message.
 * OpenRouter ids carry a vendor prefix and spell versions with dots
 * (`anthropic/claude-sonnet-4.6`); unknown ids are shown as they are.
 * @param {string | null | undefined} model
 */
export function modelShortLabel(model) {
	if (!model) return "";
	const bare = model.includes("/") ? model.slice(model.indexOf("/") + 1) : model;
	const claude = /^claude-(opus|sonnet|haiku)-(\d+)[.-](\d+)(?:-\d{8})?$/.exec(bare);
	if (claude) {
		const family = claude[1][0].toUpperCase() + claude[1].slice(1);
		return `${family} ${claude[2]}.${claude[3]}`;
	}
	const gpt = /^gpt-(\d+(?:\.\d+)?)(?:-(mini|nano))?$/.exec(bare);
	if (gpt) return gpt[2] ? `GPT-${gpt[1]} ${gpt[2]}` : `GPT-${gpt[1]}`;
	return model;
}

/**
 * A stable id from a display name, unique among `existing`.
 * @param {string} name
 * @param {string[]} existing
 */
export function suggestPresetId(name, existing) {
	const base =
		name
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, "-")
			.replace(/^-+|-+$/g, "") || "preset";
	if (!existing.includes(base)) return base;
	let n = 2;
	while (existing.includes(`${base}-${n}`)) n += 1;
	return `${base}-${n}`;
}

/**
 * Presets grouped by provider, in provider order, skipping empty groups.
 * @param {ModelPreset[]} presets
 * @returns {{ provider: Provider, presets: ModelPreset[] }[]}
 */
export function presetsByProvider(presets) {
	return PROVIDERS.map((provider) => ({ provider, presets: presets.filter((p) => p.provider === provider.id) })).filter(
		(group) => group.presets.length > 0,
	);
}
