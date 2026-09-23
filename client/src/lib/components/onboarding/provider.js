import { codexView } from "../../models/codex.js";
import { presetTestCopy, providerAuth, providerLabel } from "../../models/presets.js";

/**
 * @typedef {import("../../models/presets.js").ModelPreset} ModelPreset
 * @typedef {import("../../models/presets.js").PresetTestOk} PresetTestOk
 * @typedef {import("../../models/presets.js").PresetTestFailure} PresetTestFailure
 * @typedef {import("../../models/codex.js").CodexStatus} CodexStatus
 * @typedef {'anthropic' | 'openai' | 'openrouter' | 'codex'} OnboardingProvider
 * @typedef {{ id: string, name: string, description?: string }} AvailableModel
 */

/**
 * The step after a provider is picked: its key, unless one is already
 * saved (a key from an earlier visit, or one the server reads from its
 * environment), the Codex login, or straight to its models.
 * @param {OnboardingProvider} provider
 * @param {readonly string[] | null | undefined} configuredKeys `configured_keys` from the config status
 * @returns {'key' | 'codex' | 'models'}
 */
export function stepAfterProvider(provider, configuredKeys) {
	if (providerAuth(provider) === "login") return "codex";
	return configuredKeys?.includes(provider) ? "models" : "key";
}

/**
 * Save the provider's key. The server probes it before saving (a wrong key
 * is refused with the reason), and nothing else changes: the person picks
 * a model next.
 * @param {'anthropic' | 'openai' | 'openrouter'} provider
 * @param {string} key
 * @param {{ updateLlmConfig: (payload: {api_key?: string, openai?: string, openrouter?: string}) => Promise<void> }} api
 */
export async function saveOnboardingKey(provider, key, api) {
	const payload = provider === "openai" ? { openai: key } : provider === "openrouter" ? { openrouter: key } : { api_key: key };
	await api.updateLlmConfig(payload);
}

/**
 * The models the provider offers this account, to pick one from. A refusal
 * rejects with the sentence onboarding shows and the typed outcome on
 * `error.outcome`; a listing with nothing in it rejects too, since there is
 * nothing to pick.
 * @param {OnboardingProvider} provider
 * @param {{ fetchAvailableModels: (provider: OnboardingProvider) => Promise<{ ok: true, models: AvailableModel[] } | PresetTestFailure> }} api
 * @returns {Promise<AvailableModel[]>}
 */
export async function listOnboardingModels(provider, api) {
	const listed = await api.fetchAvailableModels(provider);
	if (!listed.ok) {
		const row = { id: "", name: "", provider, model: "" };
		const error = /** @type {Error & { outcome?: PresetTestFailure }} */ (new Error(presetTestCopy(listed, row).text));
		error.outcome = listed;
		throw error;
	}
	if (listed.models.length === 0) throw new Error(`${providerLabel(provider)} lists no models this account can use.`);
	return listed.models;
}

/**
 * Make the picked model the one conversations use (#28): the server tests
 * it and saves nothing unless it answers, so onboarding cannot finish on a
 * model that does not. A refusal rejects with the connection test's
 * sentence and the typed outcome on `error.outcome`; the person can pick
 * another model or another provider.
 * @param {OnboardingProvider} provider
 * @param {AvailableModel} model
 * @param {{ chooseModel: (choice: { provider: OnboardingProvider, model: string, name?: string }) => Promise<PresetTestOk | PresetTestFailure> }} api
 * @returns {Promise<PresetTestOk>}
 */
export async function chooseOnboardingModel(provider, model, api) {
	const outcome = await api.chooseModel({ provider, model: model.id, name: model.name });
	if (!outcome.ok) {
		const row = { id: "", name: model.name, provider, model: model.id };
		const error = /** @type {Error & { outcome?: PresetTestFailure }} */ (new Error(presetTestCopy(outcome, row).text));
		error.outcome = outcome;
		throw error;
	}
	return outcome;
}

/**
 * Check Codex before its models are listed (#27): the credential lives
 * with codex, so nothing is saved here. A binary that is missing or another
 * release (`error.codex === 'binary'`), an app-server that could not answer
 * (`'unavailable'`) or no login yet (`'login'`, the caller starts one)
 * rejects; a login resolves with the status.
 * @param {{ fetchCodexStatus: () => Promise<CodexStatus> }} api
 * @returns {Promise<CodexStatus>}
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
	return status;
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
