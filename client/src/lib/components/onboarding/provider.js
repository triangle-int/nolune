/**
 * Save the credential in its provider's slot, then seed that provider's
 * default model presets (#156) so chat and background work have a model.
 * @param {'anthropic' | 'openai' | 'openrouter'} provider
 * @param {string} key
 * @param {{ updateLlmConfig: (payload: {api_key?: string, openai?: string, openrouter?: string}) => Promise<void>, seedModelPresets: (provider: 'anthropic' | 'openai' | 'openrouter') => Promise<unknown> }} api
 */
export async function saveOnboardingProvider(provider, key, api) {
	const payload = provider === 'openai' ? { openai: key } : provider === 'openrouter' ? { openrouter: key } : { api_key: key };
	await api.updateLlmConfig(payload);
	await api.seedModelPresets(provider);
}
