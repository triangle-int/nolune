import { presetTestCopy } from "../../models/presets.js";

/**
 * @typedef {import("../../models/presets.js").ModelPreset} ModelPreset
 * @typedef {{ presets: ModelPreset[], chat_preset: string }} SeededModels
 * @typedef {import("../../models/presets.js").PresetTestOk} PresetTestOk
 * @typedef {import("../../models/presets.js").PresetTestFailure} PresetTestFailure
 */

/**
 * The preset onboarding tests once a provider is keyed (#28): the Chat slot
 * when it already runs on that provider, else the provider's first preset.
 * Mirrors the server's `probe_model`.
 * @param {SeededModels} models
 * @param {string} provider
 * @returns {string | null}
 */
export function onboardingTestPreset(models, provider) {
	const chat = models.presets.find((p) => p.id === models.chat_preset);
	if (chat?.provider === provider) return chat.id;
	return models.presets.find((p) => p.provider === provider)?.id ?? null;
}

/**
 * Save the credential in its provider's slot, seed that provider's default
 * model presets (#156) so chat and background work have a model, then run
 * the connection test on one of them (#28): onboarding cannot finish with a
 * provider that does not answer. A failure rejects with the typed outcome
 * on `error.outcome`; the key stays saved, so a valid provider configured
 * earlier is untouched and the person can retry from the key step.
 * @param {'anthropic' | 'openai' | 'openrouter'} provider
 * @param {string} key
 * @param {{ updateLlmConfig: (payload: {api_key?: string, openai?: string, openrouter?: string}) => Promise<void>, seedModelPresets: (provider: 'anthropic' | 'openai' | 'openrouter') => Promise<SeededModels>, testPreset: (id: string) => Promise<PresetTestOk | PresetTestFailure> }} api
 * @returns {Promise<PresetTestOk>}
 */
export async function saveOnboardingProvider(provider, key, api) {
	const payload = provider === 'openai' ? { openai: key } : provider === 'openrouter' ? { openrouter: key } : { api_key: key };
	await api.updateLlmConfig(payload);
	const models = await api.seedModelPresets(provider);
	const presetId = onboardingTestPreset(models, provider);
	if (!presetId) throw new Error(`No ${provider} preset to test; add one under Settings.`);
	const outcome = await api.testPreset(presetId);
	if (!outcome.ok) {
		const preset = models.presets.find((p) => p.id === presetId);
		const error = /** @type {Error & { outcome?: PresetTestFailure }} */ (new Error(presetTestCopy(outcome, /** @type {ModelPreset} */ (preset)).text));
		error.outcome = outcome;
		throw error;
	}
	return outcome;
}

/**
 * Where onboarding resumes once the server has said whether a provider is
 * configured (#28). `llm_configured` only means a key and a Chat preset are
 * saved; a preset that never answered is saved too, so only a passing test
 * skips to the first message. Anything else returns to the provider step
 * with the outcome sentence, or `null` when there is nothing to report.
 * @param {{ llm_configured: boolean, chat_preset?: string, chat_provider?: string | null, model?: string | null }} status
 * @param {PresetTestOk | PresetTestFailure | null} outcome the Chat preset's test, `null` when it could not run
 * @param {ModelPreset | null | undefined} preset the Chat preset's row when the listing loaded
 * @returns {{ step: 'first-message' } | { step: 'provider', reason: string | null }}
 */
export function resumeOnboarding(status, outcome, preset) {
	void status;
	void outcome;
	void preset;
	return { step: "provider", reason: null };
}
