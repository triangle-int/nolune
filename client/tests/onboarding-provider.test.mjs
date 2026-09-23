import test from 'node:test';
import assert from 'node:assert/strict';
import { chooseOnboardingModel, connectOnboardingCodex, listOnboardingModels, resumeOnboarding, saveOnboardingKey, stepAfterProvider } from '../src/lib/components/onboarding/provider.js';

for (const [provider, field] of [['openai', 'openai'], ['anthropic', 'api_key'], ['openrouter', 'openrouter']]) {
	test(`${provider} onboarding saves only its own credential`, async () => {
		const calls = [];
		await saveOnboardingKey(provider, 'test-only-key', { updateLlmConfig: async (payload) => { calls.push(payload); } });
		assert.deepEqual(calls, [{ [field]: 'test-only-key' }]);
	});
}

test('a refused key rejects with the server\'s reason', async () => {
	await assert.rejects(saveOnboardingKey('openai', 'test-only-key', { updateLlmConfig: async () => { throw new Error('invalid API key'); } }), /invalid API key/);
});

test('a provider whose key is already saved goes straight to its models; codex logs in first', () => {
	assert.equal(stepAfterProvider('anthropic', []), 'key');
	assert.equal(stepAfterProvider('anthropic', null), 'key');
	assert.equal(stepAfterProvider('openrouter', ['anthropic']), 'key');
	assert.equal(stepAfterProvider('openrouter', ['openrouter', 'elevenlabs']), 'models');
	assert.equal(stepAfterProvider('codex', ['openai']), 'codex');
});

const models = [
	{ id: 'claude-opus-5-5', name: 'Claude Opus 5.5' },
	{ id: 'claude-sonnet-5', name: 'Claude Sonnet 5' },
];

test('onboarding offers the models the provider lists, in its order', async () => {
	const asked = [];
	const listed = await listOnboardingModels('anthropic', { fetchAvailableModels: async (provider) => { asked.push(provider); return { ok: true, models }; } });
	assert.deepEqual(asked, ['anthropic']);
	assert.deepEqual(listed, models);
});

test('a listing the provider refuses or leaves empty keeps onboarding on the model step with what went wrong', async () => {
	const refused = await listOnboardingModels('openai', {
		fetchAvailableModels: async () => ({ ok: false, error: 'authentication', message: 'OpenAI rejected the API key.', status: 401 }),
	}).then(() => null, (e) => e);
	assert.ok(refused instanceof Error);
	assert.equal(refused.outcome.error, 'authentication');
	assert.match(refused.message, /OpenAI rejected the API key/);
	await assert.rejects(listOnboardingModels('openrouter', { fetchAvailableModels: async () => ({ ok: true, models: [] }) }), /OpenRouter lists no models/);
	await assert.rejects(listOnboardingModels('codex', { fetchAvailableModels: async () => ({ ok: false, error: 'unreachable', message: 'failed to reach Codex: closed', status: 502 }) }), /failed to reach Codex/);
});

test('a picked model is chosen by provider, id and name; the server tests it before saving (#28)', async () => {
	const calls = [];
	const result = await chooseOnboardingModel('anthropic', models[1], {
		chooseModel: async (choice) => { calls.push(choice); return { ok: true, preset: 'claude-sonnet-5', provider: 'anthropic', model: 'claude-sonnet-5', usage: { input_tokens: 8, output_tokens: 1 } }; },
	});
	assert.deepEqual(calls, [{ provider: 'anthropic', model: 'claude-sonnet-5', name: 'Claude Sonnet 5' }]);
	assert.equal(result.preset, 'claude-sonnet-5');
});

test('a picked model that does not answer rejects with the typed outcome, naming the model', async () => {
	for (const [error, message, pattern] of [
		['model_not_found', 'Anthropic has no model "claude-sonnet-5"', /no model "claude-sonnet-5"/],
		['provider_rejected', 'Anthropic rejected the request (400): Your credit balance is too low', /credit balance/],
		['rate_limited', 'Anthropic accepted the key but is rate limiting', /rate limiting/],
	]) {
		const failure = await chooseOnboardingModel('anthropic', models[1], {
			chooseModel: async () => ({ ok: false, error, message, status: 422 }),
		}).then(() => null, (e) => e);
		assert.ok(failure instanceof Error, error);
		assert.equal(failure.outcome.error, error);
		assert.match(failure.message, pattern, `${error}: ${failure.message}`);
	}
});

// --- the gate on a reload (#28) ---
// Once a key is saved and a model was picked, the server reports
// `llm_configured` even when the preset never answered; onboarding must
// test again instead of trusting the flag.

const gpt = { id: 'gpt-sol', name: 'GPT-6 Sol', provider: 'openai', model: 'gpt-6-sol' };

test('a companion without a provider goes to the provider step', () => {
	assert.deepEqual(resumeOnboarding({ llm_configured: false }, null, null), { step: 'provider', reason: null });
	assert.deepEqual(resumeOnboarding({ llm_configured: false, chat_preset: 'gpt-sol' }, { ok: true, preset: 'gpt-sol', provider: 'openai', model: 'gpt-6-sol', usage: { input_tokens: 1, output_tokens: 1 } }, gpt), { step: 'provider', reason: null });
});

test('a configured provider skips to the first message only when its Chat preset answered', () => {
	const ok = { ok: true, preset: 'gpt-sol', provider: 'openai', model: 'gpt-6-sol', usage: { input_tokens: 8, output_tokens: 1 } };
	assert.deepEqual(resumeOnboarding({ llm_configured: true, chat_preset: 'gpt-sol' }, ok, gpt), { step: 'first-message' });
});

test('a configured provider whose preset does not answer returns to the provider step with the typed outcome', () => {
	for (const [error, message, pattern] of [
		['model_not_found', 'OpenAI has no model "gpt-6-sol": does not exist', /no model "gpt-6-sol"/],
		['rate_limited', 'OpenAI accepted the key but is rate limiting', /rate limiting/],
		['provider_rejected', 'OpenAI rejected the request (402): Insufficient credits', /Insufficient credits/],
		['authentication', 'OpenAI rejected the API key.', /rejected the API key/],
	]) {
		const next = resumeOnboarding({ llm_configured: true, chat_preset: 'gpt-sol' }, { ok: false, error, message, status: 422 }, gpt);
		assert.equal(next.step, 'provider', error);
		assert.match(next.reason, pattern, `${error}: ${next.reason}`);
	}
	// A test that could not run at all is not a pass either.
	const failed = resumeOnboarding({ llm_configured: true, chat_preset: 'gpt-sol' }, null, gpt);
	assert.equal(failed.step, 'provider');
	assert.match(failed.reason, /could not be tested/i);
	// Without the preset row, the sentence still names the provider from the status.
	const bare = resumeOnboarding({ llm_configured: true, chat_preset: 'gpt-sol', chat_provider: 'openai', model: 'gpt-6-sol' }, { ok: false, error: 'model_not_found', message: 'nope', status: 404 }, null);
	assert.match(bare.reason, /OpenAI has no model "gpt-6-sol"/);
});

// --- Codex (#27): the gate is the login, then a model that answers ---

const codexStatus = (over = {}) => ({
	binary: { state: 'ready', pinned_version: '0.156.1', path: '/opt/homebrew/bin/codex', version: '0.156.1' },
	installed: true,
	compatible: true,
	logged_in: true,
	account: { kind: 'chatgpt', email: 'companion@example.test', plan: 'plus' },
	login: null,
	...over,
});

test('codex onboarding goes on to its models once codex holds a login (#27)', async () => {
	const calls = [];
	const status = await connectOnboardingCodex({ fetchCodexStatus: async () => { calls.push('status'); return codexStatus(); } });
	// No key is ever saved: the login lives in codex.
	assert.deepEqual(calls, ['status']);
	assert.equal(status.logged_in, true);
});

test('codex onboarding stops when the binary is missing, mismatched, silent, or holds no login', async () => {
	for (const [name, status, kind, pattern] of [
		['missing', codexStatus({ binary: { state: 'not_installed', pinned_version: '0.156.1', message: 'codex is not installed: no `codex` on PATH and NOLUNE_CODEX_BIN is unset' }, installed: false, compatible: false, logged_in: false, account: null }), 'binary', /not installed/i],
		['mismatched', codexStatus({ binary: { state: 'incompatible', pinned_version: '0.156.1', path: '/usr/local/bin/codex', version: '0.155.0', message: '/usr/local/bin/codex is codex 0.155.0; Nolune supports codex 0.156.1 only' }, compatible: false, logged_in: false, account: null }), 'binary', /0\.155\.0.*0\.156\.1/],
		['silent', codexStatus({ logged_in: false, account: null, error: 'codex app-server handshake failed: exited with status 1' }), 'unavailable', /handshake failed/],
		['logged out', codexStatus({ logged_in: false, account: null }), 'login', /not logged in/i],
	]) {
		const error = await connectOnboardingCodex({ fetchCodexStatus: async () => status }).then(() => null, (e) => e);
		assert.ok(error instanceof Error, name);
		assert.equal(error.codex, kind, name);
		assert.match(error.message, pattern, `${name}: ${error.message}`);
	}
});

test('a codex model that does not answer keeps onboarding open with the login sentence, not a key one', async () => {
	const error = await chooseOnboardingModel('codex', { id: 'gpt-6-astra', name: 'GPT-6-Astra' }, {
		chooseModel: async () => ({ ok: false, error: 'setup_required', message: 'Codex login required: sign in with ChatGPT from Settings › Connections, or run `codex login` on this machine.', status: 503 }),
	}).then(() => null, (e) => e);
	assert.ok(error instanceof Error);
	assert.equal(error.outcome.error, 'setup_required');
	assert.match(error.message, /Codex login required/);
	assert.doesNotMatch(error.message, /API key/);
	assert.equal(error.codex, undefined, 'the outcome is the test\'s, not the gate\'s');
});

test('a configured codex preset whose login is gone returns to the provider step with the login sentence', () => {
	const sol = { id: 'codex-sol', name: 'GPT-6 Sol via Codex', provider: 'codex', model: 'gpt-6-sol' };
	const outcome = { ok: false, error: 'setup_required', message: 'Codex login required: sign in with ChatGPT from Settings › Connections, or run `codex login` on this machine.', status: 503 };
	const next = resumeOnboarding({ llm_configured: true, chat_preset: 'codex-sol', chat_provider: 'codex' }, outcome, sol);
	assert.equal(next.step, 'provider');
	assert.match(next.reason, /Codex login required/);
	assert.doesNotMatch(next.reason, /API key/);
	assert.deepEqual(resumeOnboarding({ llm_configured: true, chat_preset: 'codex-sol' }, { ok: true, preset: 'codex-sol', provider: 'codex', model: 'gpt-6-sol', usage: { input_tokens: 1, output_tokens: 1 } }, sol), { step: 'first-message' });
});
