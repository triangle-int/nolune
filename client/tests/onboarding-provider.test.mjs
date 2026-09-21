import test from 'node:test';
import assert from 'node:assert/strict';
import { onboardingTestPreset, resumeOnboarding, saveOnboardingProvider, slotsAfterOnboardingTest } from '../src/lib/components/onboarding/provider.js';

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
			updateModelPresets: async payload => { calls.push(['slots', payload.chat_preset, payload.background_preset]); return { ...seeded(provider), ...payload }; },
		});
		// The slots follow the preset that answered when they pointed at the
		// provider being left (both sat on Anthropic's sonnet, and the fixture
		// seeds one preset per provider); Anthropic's already was the Chat
		// slot, so nothing moves.
		const expected = [['save', { [field]: 'test-only-key' }], ['seed', provider], ['test', preset]];
		if (provider !== 'anthropic') expected.push(['slots', preset, preset]);
		assert.deepEqual(calls, expected);
		assert.equal(result.ok, true);
		assert.equal(result.preset, preset);
	});
}

test('the slots follow the provider that answered when the one they pointed at did not (#28)', () => {
	// A first provider whose key passed the probe but whose model never
	// answered took both slots; the next provider's preset answers.
	const models = {
		presets: [
			{ id: 'gpt', name: 'GPT-5.4', provider: 'openai', model: 'gpt-5.4' },
			{ id: 'gpt-mini', name: 'GPT-5.4 mini', provider: 'openai', model: 'gpt-5.4-mini' },
			{ id: 'sonnet', name: 'Claude Sonnet', provider: 'anthropic', model: 'claude-sonnet-4-6' },
			{ id: 'opus', name: 'Claude Opus', provider: 'anthropic', model: 'claude-opus-4-6' },
			{ id: 'haiku', name: 'Claude Haiku', provider: 'anthropic', model: 'claude-haiku-4-5-20251001' },
		],
		chat_preset: 'gpt',
		background_preset: 'gpt-mini',
	};
	assert.deepEqual(slotsAfterOnboardingTest(models, 'sonnet'), { chat_preset: 'sonnet', background_preset: 'opus' });
	// The tested preset already is the Chat slot: nothing moves.
	assert.equal(slotsAfterOnboardingTest({ ...models, chat_preset: 'sonnet' }, 'sonnet'), null);
	// The Background slot stays wherever it is not on the provider being left.
	assert.deepEqual(slotsAfterOnboardingTest({ ...models, background_preset: 'haiku' }, 'sonnet'), { chat_preset: 'sonnet', background_preset: 'haiku' });
	// A provider with one preset backs both slots with it.
	const single = { ...models, presets: models.presets.slice(0, 3) };
	assert.deepEqual(slotsAfterOnboardingTest(single, 'sonnet'), { chat_preset: 'sonnet', background_preset: 'sonnet' });
	// A slot pointing nowhere is filled too.
	assert.deepEqual(slotsAfterOnboardingTest({ ...models, chat_preset: '', background_preset: '' }, 'sonnet'), { chat_preset: 'sonnet', background_preset: 'opus' });
});

test('slots move only after the preset answered, and a failed move is reported', async () => {
	let moved = false;
	await assert.rejects(saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => {},
		seedModelPresets: async (provider) => seeded(provider),
		testPreset: async () => ({ ok: false, error: 'provider_rejected', message: 'OpenAI rejected the request (402): Insufficient credits', status: 422 }),
		updateModelPresets: async () => { moved = true; },
	}), /Insufficient credits/);
	assert.equal(moved, false);
	await assert.rejects(saveOnboardingProvider('openai', 'test-only-key', {
		updateLlmConfig: async () => {},
		seedModelPresets: async (provider) => seeded(provider),
		testPreset: async (id) => ({ ok: true, preset: id, provider: 'openai', model: 'gpt-5.4', usage: { input_tokens: 1, output_tokens: 1 } }),
		updateModelPresets: async () => { throw new Error('Could not save the model slots'); },
	}), /Could not save the model slots/);
});

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

// --- the gate on a reload (#28) ---
// Once a key is saved and presets are seeded, the server reports
// `llm_configured` even when the preset never answered; onboarding must
// test again instead of trusting the flag.

const gpt = { id: 'gpt', name: 'GPT-5.4', provider: 'openai', model: 'gpt-5.4' };

test('a companion without a provider goes to the provider step', () => {
	assert.deepEqual(resumeOnboarding({ llm_configured: false }, null, null), { step: 'provider', reason: null });
	assert.deepEqual(resumeOnboarding({ llm_configured: false, chat_preset: 'gpt' }, { ok: true, preset: 'gpt', provider: 'openai', model: 'gpt-5.4', usage: { input_tokens: 1, output_tokens: 1 } }, gpt), { step: 'provider', reason: null });
});

test('a configured provider skips to the first message only when its Chat preset answered', () => {
	const ok = { ok: true, preset: 'gpt', provider: 'openai', model: 'gpt-5.4', usage: { input_tokens: 8, output_tokens: 1 } };
	assert.deepEqual(resumeOnboarding({ llm_configured: true, chat_preset: 'gpt' }, ok, gpt), { step: 'first-message' });
});

test('a configured provider whose preset does not answer returns to the provider step with the typed outcome', () => {
	for (const [error, message, pattern] of [
		['model_not_found', 'OpenAI has no model "gpt-5.4": does not exist', /no model "gpt-5\.4"/],
		['rate_limited', 'OpenAI accepted the key but is rate limiting', /rate limiting/],
		['provider_rejected', 'OpenAI rejected the request (402): Insufficient credits', /Insufficient credits/],
		['authentication', 'OpenAI rejected the API key.', /rejected the API key/],
	]) {
		const next = resumeOnboarding({ llm_configured: true, chat_preset: 'gpt' }, { ok: false, error, message, status: 422 }, gpt);
		assert.equal(next.step, 'provider', error);
		assert.match(next.reason, pattern, `${error}: ${next.reason}`);
	}
	// A test that could not run at all is not a pass either.
	const failed = resumeOnboarding({ llm_configured: true, chat_preset: 'gpt' }, null, gpt);
	assert.equal(failed.step, 'provider');
	assert.match(failed.reason, /could not be tested/i);
	// Without the preset row, the sentence still names the provider from the status.
	const bare = resumeOnboarding({ llm_configured: true, chat_preset: 'gpt', chat_provider: 'openai', model: 'gpt-5.4' }, { ok: false, error: 'model_not_found', message: 'nope', status: 404 }, null);
	assert.match(bare.reason, /OpenAI has no model "gpt-5\.4"/);
});
