/**
 * Model presets (#156): the user names the models Nolune may call. Two slots
 * say which preset handles conversations and which does background work, and
 * a chat can pin its own. This module is pure so its rules are testable.
 *
 * @typedef {{ id: string, label: string }} Provider
 * @typedef {{ id: string, name: string, provider: string, model: string }} ModelPreset
 * @typedef {{ chat_preset: string, background_preset: string }} Slots
 * @typedef {{ vision: boolean, documents: boolean, tools: boolean }} Capabilities
 * @typedef {{ id: 'vision' | 'documents' | 'tools', chip: string, detail: string }} CapabilityWarning
 * @typedef {{ ok: true, preset: string, provider: string, model: string, usage: { input_tokens: number, output_tokens: number } }} PresetTestOk
 * @typedef {{ ok: false, error: string, message: string, status: number, retry_after_seconds?: number | null }} PresetTestFailure
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

/**
 * What a preset's model cannot do (#28), as a chip for the row and a
 * sentence for the warning shown before the model is selected. Unknown
 * capabilities (a preset not saved yet) warn about nothing.
 *
 * @param {ModelPreset} preset
 * @param {Capabilities | null | undefined} caps what the server reports for this preset
 * @returns {CapabilityWarning[]}
 */
export function capabilityWarnings(preset, caps) {
	if (!caps) return [];
	const name = (preset?.name ?? "").trim() || preset?.model || "This model";
	/** @type {CapabilityWarning[]} */
	const warnings = [];
	if (!caps.vision) warnings.push({ id: "vision", chip: "no vision", detail: `${name} cannot see images: screenshots and photos sent to it are refused.` });
	if (!caps.documents) warnings.push({ id: "documents", chip: "no documents", detail: `${name} cannot read PDFs and other documents; share them as text instead.` });
	if (!caps.tools) warnings.push({ id: "tools", chip: "no tools", detail: `${name} cannot call tools, so with it the companion cannot act on computers, search, or use extensions.` });
	return warnings;
}

/**
 * The capabilities `GET /api/config/models` reports for one preset id.
 * @param {{ capabilities?: Record<string, Capabilities> } | null | undefined} models
 * @param {string} id
 * @returns {Capabilities | undefined}
 */
export function presetCapabilities(models, id) {
	return models?.capabilities?.[id];
}

/**
 * One sentence for a connection test outcome (#28), keyed on the typed
 * `error` the server answers with. The server's message already names the
 * provider and what it said; the copy here adds what to do about it, and
 * an error the client does not know shows the message as it is.
 *
 * @param {PresetTestOk | PresetTestFailure} outcome
 * @param {ModelPreset} preset
 * @returns {{ tone: 'ok' | 'error', text: string }}
 */
export function presetTestCopy(outcome, preset) {
	const provider = providerLabel(preset?.provider);
	const model = preset?.model || "the model";
	if (outcome.ok) {
		const tokens = (outcome.usage?.input_tokens ?? 0) + (outcome.usage?.output_tokens ?? 0);
		return { tone: "ok", text: `${outcome.model || model} answered · ${tokens} tokens used.` };
	}
	const said = outcome.message?.trim() || `${provider} did not answer.`;
	const wait = outcome.retry_after_seconds ? `in ${outcome.retry_after_seconds} s` : "in a moment";
	/** @type {Record<string, string>} */
	const copy = {
		setup_required: `No ${provider} API key yet. Add one under API keys, then test again.`,
		authentication: `${provider} rejected the API key. Change it under API keys.`,
		rate_limited: `${provider} accepted the key but is rate limiting right now; the key works, try again ${wait}.`,
		model_not_found: `${provider} has no model "${model}". Check the model id.`,
		provider_rejected: said,
		provider_unavailable: `${said}. Try again in a moment.`,
		unreachable: `${said}. Check that this server can reach the internet.`,
		timeout: `${provider} did not answer in time. Try again.`,
		invalid_response: said,
		unsupported: said,
		unknown_preset: "Save the preset first, then test it.",
	};
	return { tone: "error", text: copy[outcome.error] ?? said };
}

/**
 * @typedef {ModelPreset & { warnings: CapabilityWarning[] }} PickerPreset
 */

/**
 * The presets the composer picker offers (#28): each row with what its
 * model cannot do, so the chips show before a model is chosen and the
 * sentence under the composer once it is. A listing without capabilities
 * (an older server) warns about nothing.
 * @param {{ presets?: ModelPreset[], capabilities?: Record<string, Capabilities> } | null | undefined} models
 * @returns {PickerPreset[]}
 */
export function pickerPresets(models) {
	void models;
	return [];
}
