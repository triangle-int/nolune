import test from 'node:test';
import assert from 'node:assert/strict';
import { saveOnboardingProvider } from '../src/lib/components/onboarding/provider.js';

for (const [provider, field] of [['openai', 'openai'], ['anthropic', 'api_key'], ['openrouter', 'openrouter']]) {
	test(`${provider} onboarding saves only its own credential and seeds its presets afterward`, async () => {
		const calls = [];
		await saveOnboardingProvider(provider, 'test-only-key', {
			updateLlmConfig: async payload => { calls.push(['save', payload]); },
			seedModelPresets: async value => { calls.push(['seed', value]); },
		});
		assert.deepEqual(calls, [['save', { [field]: 'test-only-key' }], ['seed', provider]]);
	});
}

test('failed credential save does not seed presets', async () => {
	let activated = false;
	await assert.rejects(saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => { throw new Error('Save failed'); },
		seedModelPresets: async () => { activated = true; },
	}), /Save failed/);
	assert.equal(activated, false);
});

test('seeding failure is returned to onboarding instead of reporting success', async () => {
	await assert.rejects(saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => {},
		seedModelPresets: async () => { throw new Error('Seeding failed'); },
	}), /Seeding failed/);
});
