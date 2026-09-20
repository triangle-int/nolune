import test from 'node:test';
import assert from 'node:assert/strict';
import { onboardingTestPreset, saveOnboardingProvider } from '../src/lib/components/onboarding/provider.js';

const seeded = (provider) => ({
	presets: [
		{ id: 'sonnet', name: 'Claude Sonnet', provider: 'anthropic', model: 'claude-sonnet-4-6' },
		{ id: 'gpt', name: 'GPT-5.4', provider: 'openai', model: 'gpt-5.4' },
		{ id: 'openrouter-sonnet', name: 'Sonnet via OpenRouter', provider: 'openrouter', model: 'anthropic/claude-sonnet-4.6' },
	].filter((p) => p.provider === provider || p.provider === 'anthropic'),
	chat_preset: 'sonnet',
	background_preset: 'sonnet',
	keyed_providers: ['anthropic', provider],
	setup_required: null,
	added: 2,
});

for (const [provider, field, preset] of [['openai', 'openai', 'gpt'], ['anthropic', 'api_key', 'sonnet'], ['openrouter', 'openrouter', 'openrouter-sonnet']]) {
	test(`${provider} onboarding saves only its own credential, seeds its presets, then tests the new provider's preset (#28)`, async () => {
		const calls = [];
		const result = await saveOnboardingProvider(provider, 'test-only-key', {
			updateLlmConfig: async payload => { calls.push(['save', payload]); },
			seedModelPresets: async value => { calls.push(['seed', value]); return seeded(value); },
			testPreset: async id => { calls.push(['test', id]); return { ok: true, preset: id, provider, model: 'm', usage: { input_tokens: 1, output_tokens: 1 } }; },
		});
		assert.deepEqual(calls, [['save', { [field]: 'test-only-key' }], ['seed', provider], ['test', preset]]);
		assert.equal(result.ok, true);
		assert.equal(result.preset, preset);
	});
}

test('failed credential save does not seed presets or test anything', async () => {
	let activated = false;
	await assert.rejects(saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => { throw new Error('Save failed'); },
		seedModelPresets: async () => { activated = true; },
		testPreset: async () => { activated = true; },
	}), /Save failed/);
	assert.equal(activated, false);
});

test('seeding failure is returned to onboarding instead of reporting success', async () => {
	let tested = false;
	await assert.rejects(saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => {},
		seedModelPresets: async () => { throw new Error('Seeding failed'); },
		testPreset: async () => { tested = true; },
	}), /Seeding failed/);
	assert.equal(tested, false);
});

test('a preset that does not answer keeps onboarding on the key step with the typed outcome (#28)', async () => {
	// The key was accepted, so it is saved; the person still cannot finish
	// until a preset of that provider answers.
	const error = await saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => {},
		seedModelPresets: async (provider) => seeded(provider),
		testPreset: async () => ({ ok: false, error: 'model_not_found', message: 'OpenAI has no model gpt-5.4', status: 404 }),
	}).then(() => null, (e) => e);
	assert.ok(error instanceof Error);
	assert.match(error.message, /gpt-5\.4|OpenAI/);
	assert.equal(error.outcome.error, 'model_not_found');
});

test('the preset onboarding tests is the new provider\'s, the Chat slot when it already runs there', () => {
	const models = seeded('openai');
	assert.equal(onboardingTestPreset(models, 'openai'), 'gpt');
	assert.equal(onboardingTestPreset(models, 'anthropic'), 'sonnet');
	assert.equal(onboardingTestPreset({ ...models, chat_preset: 'gpt' }, 'openai'), 'gpt');
	assert.equal(onboardingTestPreset({ ...models, presets: [] }, 'openai'), null);
});
