import { codexView } from "../../models/codex.js";
import { presetTestCopy, providerLabel } from "../../models/presets.js";

/**
 * @typedef {import("../../models/presets.js").ModelPreset} ModelPreset
 * @typedef {{ presets: ModelPreset[], chat_preset: string, background_preset: string }} SeededModels
 * @typedef {import("../../models/presets.js").PresetTestOk} PresetTestOk
 * @typedef {import("../../models/presets.js").PresetTestFailure} PresetTestFailure
 * @typedef {import("../../models/codex.js").CodexStatus} CodexStatus
 * @typedef {'anthropic' | 'openai' | 'openrouter' | 'codex'} SeedProvider
 * @typedef {{ seedModelPresets: (provider: SeedProvider) => Promise<SeededModels>, testPreset: (id: string) => Promise<PresetTestOk | PresetTestFailure>, updateModelPresets: (payload: { presets: ModelPreset[], chat_preset: string, background_preset: string }) => Promise<unknown> }} SeedAndTestApi
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
 * Where the slots go once `presetId` answered (#28). Onboarding only reaches
 * a key step while the Chat slot points nowhere or at a preset that did not
 * answer (a key can pass the probe and still have no usable model: no
 * credits, a rate limit, a retired id), so a preset that answers takes the
 * Chat slot, and the Background slot follows to the provider's second
 * preset when it pointed at the provider being left. A slot on any other
 * provider stays. `null` when the Chat slot already is `presetId`.
 * @param {SeededModels} models
 * @param {string} presetId
 * @returns {{ chat_preset: string, background_preset: string } | null}
 */
export function slotsAfterOnboardingTest(models, presetId) {
	if (models.chat_preset === presetId) return null;
	const preset = models.presets.find((p) => p.id === presetId);
	if (!preset) return null;
	const leaving = models.presets.find((p) => p.id === models.chat_preset)?.provider;
	const background = models.presets.find((p) => p.id === models.background_preset);
	const own = models.presets.filter((p) => p.provider === preset.provider);
	const follows = !background || background.provider === leaving;
	return {
		chat_preset: presetId,
		background_preset: follows ? (own.find((p) => p.id !== presetId) ?? preset).id : background.id,
	};
}

/**
 * Save the credential in its provider's slot, seed that provider's default
 * model presets (#156) so chat and background work have a model, then run
 * the connection test on one of them (#28): onboarding cannot finish with a
 * provider that does not answer. A failure rejects with the typed outcome
 * on `error.outcome`; the key stays saved, so a valid provider configured
 * earlier is untouched and the person can retry from the key step. Once a
 * preset answered, the slots follow it (`slotsAfterOnboardingTest`) so the
 * first message does not go through the provider that did not.
 * @param {'anthropic' | 'openai' | 'openrouter'} provider
 * @param {string} key
 * @param {{ updateLlmConfig: (payload: {api_key?: string, openai?: string, openrouter?: string}) => Promise<void> } & SeedAndTestApi} api
 * @returns {Promise<PresetTestOk>}
 */
export async function saveOnboardingProvider(provider, key, api) {
	const payload = provider === 'openai' ? { openai: key } : provider === 'openrouter' ? { openrouter: key } : { api_key: key };
	await api.updateLlmConfig(payload);
	return seedAndTest(provider, api);
}

/**
 * Seed a provider's default presets and run the connection test on one of
 * them; the slots follow the preset that answered. Shared by the key
 * providers and Codex, whose credential is not saved here.
 * @param {SeedProvider} provider
 * @param {SeedAndTestApi} api
 * @returns {Promise<PresetTestOk>}
 */
async function seedAndTest(provider, api) {
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
	const slots = slotsAfterOnboardingTest(models, presetId);
	if (slots) await api.updateModelPresets({ presets: models.presets, ...slots });
	return outcome;
}

/**
 * Finish Codex onboarding (#27): the gate is the login AND the connection
 * test. Nothing is saved for the credential, which lives with codex; the
 * status is read first, and a binary that is missing or another release
 * (`error.codex === 'binary'`), an app-server that could not answer
 * (`'unavailable'`) or no login yet (`'login'`, the caller starts one)
 * rejects before anything is seeded. With a login, the Codex presets are
 * seeded and one is tested like any other provider's; a preset that does
 * not answer rejects with the typed outcome on `error.outcome`.
 * @param {{ fetchCodexStatus: () => Promise<CodexStatus> } & SeedAndTestApi} api
 * @returns {Promise<PresetTestOk>}
 */
export async function connectOnboardingCodex(api) {
	const status = await api.fetchCodexStatus();
	const view = codexView(status);
	/** @param {'binary' | 'unavailable' | 'login'} codex @param {string} message */
	const refuse = (codex, message) => {
		const error = /** @type {Error & { codex?: 'binary' | 'unavailable' | 'login' }} */ (new Error(message));
		error.codex = codex;
		return error;
	};
	if (!status.compatible) throw refuse("binary", `${view.headline} ${view.detail ?? ""}`.trim());
	if (status.error) throw refuse("unavailable", `${view.headline} ${view.detail ?? ""}`.trim());
	if (!status.logged_in) throw refuse("login", "Codex is not logged in yet.");
	return seedAndTest("codex", api);
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
	if (!status?.llm_configured) return { step: "provider", reason: null };
	if (outcome?.ok) return { step: "first-message" };
	const row = preset ?? { id: status.chat_preset ?? "", name: "", provider: status.chat_provider ?? "", model: status.model ?? "" };
	if (!outcome) {
		const who = row.provider ? providerLabel(row.provider) : "the provider";
		return { step: "provider", reason: `${who} could not be tested. Pick a provider to set up, or fix the key under Settings.` };
	}
	return { step: "provider", reason: presetTestCopy(outcome, row).text };
}
