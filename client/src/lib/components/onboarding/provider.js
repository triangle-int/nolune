/**
 * Save the credential in its provider's slot, then seed that provider's
 * default model presets (#156) so chat and background work have a model.
 * @param {'anthropic' | 'openai'} provider
 * @param {string} key
 * @param {{ updateLlmConfig: (payload: {api_key?: string, openai?: string}) => Promise<void>, seedModelPresets: (provider: 'anthropic' | 'openai') => Promise<unknown> }} api
 */
export async function saveOnboardingProvider(provider, key, api) {
	await api.updateLlmConfig(provider === 'openai' ? { openai: key } : { api_key: key });
	await api.seedModelPresets(provider);
}
