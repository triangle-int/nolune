import test from 'node:test';
import assert from 'node:assert/strict';
import {
	PROVIDERS,
	capabilityWarnings,
	effectivePresetId,
	modelShortLabel,
	presetCapabilities,
	presetLabel,
	presetTestCopy,
	presetsByProvider,
	suggestPresetId,
	validatePresets,
} from '../src/lib/models/presets.js';

const presets = [
	{ id: 'sonnet', name: 'Claude Sonnet', provider: 'anthropic', model: 'claude-sonnet-4-6' },
	{ id: 'haiku', name: 'Claude Haiku', provider: 'anthropic', model: 'claude-haiku-4-5-20251001' },
	{ id: 'gpt', name: 'GPT-5.4', provider: 'openai', model: 'gpt-5.4' },
	{ id: 'router', name: 'Sonnet via OpenRouter', provider: 'openrouter', model: 'anthropic/claude-sonnet-4.6' },
];
const slots = { chat_preset: 'sonnet', background_preset: 'haiku' };

test('providers are the three adapters the server ships (#156, #26)', () => {
	assert.deepEqual(PROVIDERS.map((p) => p.id), ['anthropic', 'openai', 'openrouter']);
	for (const p of PROVIDERS) assert.ok(p.label);
	assert.equal(PROVIDERS.find((p) => p.id === 'openrouter').label, 'OpenRouter');
});

test('valid presets and slots produce no errors', () => {
	assert.deepEqual(validatePresets(presets, slots, ['anthropic', 'openai', 'openrouter']), []);
});

test('an OpenRouter preset names its model as vendor/model (#26)', () => {
	const bare = [{ id: 'router', name: 'Router', provider: 'openrouter', model: 'gpt-5.4' }];
	const errors = validatePresets(bare, { chat_preset: 'router', background_preset: 'router' }, ['openrouter']);
	assert.equal(errors.length, 1, errors.join('\n'));
	assert.match(errors[0], /vendor\/model/);
	for (const model of ['openai/gpt-5.4-mini', 'meta-llama/llama-4-maverick:free']) {
		const ok = [{ id: 'router', name: 'Router', provider: 'openrouter', model }];
		assert.deepEqual(validatePresets(ok, { chat_preset: 'router', background_preset: 'router' }, ['openrouter']), [], model);
	}
	// Other providers keep their plain ids.
	assert.deepEqual(validatePresets([{ id: 'gpt', name: 'GPT', provider: 'openai', model: 'gpt-5.4' }], { chat_preset: 'gpt', background_preset: 'gpt' }, ['openai']), []);
});

test('a slot on an OpenRouter preset needs the OpenRouter key', () => {
	const errors = validatePresets(presets, { chat_preset: 'router', background_preset: 'haiku' }, ['anthropic', 'openai']);
	assert.equal(errors.length, 1);
	assert.match(errors[0], /OpenRouter/);
	assert.match(errors[0], /key/i);
});

test('validation names every problem: ids, names, providers, models, slots, keys', () => {
	const errors = validatePresets(
		[
			{ id: '', name: 'Nameless id', provider: 'anthropic', model: 'x' },
			{ id: 'dup', name: 'One', provider: 'anthropic', model: 'a' },
			{ id: 'dup', name: 'Two', provider: 'openai', model: 'b' },
			{ id: 'bad', name: '', provider: 'gemini', model: '' },
		],
		{ chat_preset: 'missing', background_preset: 'dup' },
		['anthropic'],
	);
	const text = errors.join('\n');
	assert.match(text, /id/i);
	assert.match(text, /dup/);
	assert.match(text, /name/i);
	assert.match(text, /gemini/);
	assert.match(text, /model/i);
	assert.match(text, /missing/);
	assert.ok(errors.length >= 6, text);
});

test('a slot may not point at a preset whose provider has no key', () => {
	const errors = validatePresets(presets, { chat_preset: 'gpt', background_preset: 'haiku' }, ['anthropic']);
	assert.equal(errors.length, 1);
	assert.match(errors[0], /OpenAI/);
	assert.match(errors[0], /key/i);
});

test('the effective preset is the chat override, then the Chat slot, then nothing', () => {
	assert.equal(effectivePresetId('gpt', slots, presets), 'gpt');
	assert.equal(effectivePresetId('deleted', slots, presets), 'sonnet', 'a deleted override falls back to the Chat slot');
	assert.equal(effectivePresetId(null, slots, presets), 'sonnet');
	assert.equal(effectivePresetId(null, { chat_preset: 'gone', background_preset: '' }, presets), 'sonnet', 'a broken slot falls back to the first preset');
	assert.equal(effectivePresetId(null, slots, []), null);
});

test('labels stay human: preset label and short model names', () => {
	assert.equal(presetLabel(presets[0]), 'Claude Sonnet · claude-sonnet-4-6');
	assert.equal(modelShortLabel('claude-sonnet-4-6'), 'Sonnet 4.6');
	assert.equal(modelShortLabel('claude-haiku-4-5-20251001'), 'Haiku 4.5');
	assert.equal(modelShortLabel('claude-opus-4-6'), 'Opus 4.6');
	assert.equal(modelShortLabel('gpt-5.4-mini'), 'GPT-5.4 mini');
	assert.equal(modelShortLabel('gpt-5.4'), 'GPT-5.4');
	assert.equal(modelShortLabel('some-custom-model'), 'some-custom-model');
	assert.equal(modelShortLabel(''), '');
	// OpenRouter ids carry the vendor and spell versions with dots.
	assert.equal(modelShortLabel('anthropic/claude-sonnet-4.6'), 'Sonnet 4.6');
	assert.equal(modelShortLabel('openai/gpt-5.4-mini'), 'GPT-5.4 mini');
	assert.equal(modelShortLabel('meta-llama/llama-4-maverick:free'), 'meta-llama/llama-4-maverick:free');
});

test('ids are derived from names and stay unique', () => {
	assert.equal(suggestPresetId('Claude Sonnet', []), 'claude-sonnet');
	assert.equal(suggestPresetId('Claude Sonnet', ['claude-sonnet']), 'claude-sonnet-2');
	assert.equal(suggestPresetId('  GPT 5.4 mini!! ', ['gpt-5-4-mini', 'gpt-5-4-mini-2']), 'gpt-5-4-mini-3');
	assert.equal(suggestPresetId('', []), 'preset');
});

test('presets group by provider in provider order', () => {
	const grouped = presetsByProvider(presets);
	assert.deepEqual(grouped.map((g) => g.provider.id), ['anthropic', 'openai', 'openrouter']);
	assert.deepEqual(grouped[0].presets.map((p) => p.id), ['sonnet', 'haiku']);
	assert.deepEqual(grouped[2].presets.map((p) => p.id), ['router']);
	assert.deepEqual(presetsByProvider([]), []);
});

// --- capability warnings and connection tests (#28) ---

const caps = {
	sonnet: { vision: true, documents: true, tools: true, streaming: true, reasoning_controls: false, model_discovery: false, token_counting: true },
	gpt: { vision: true, documents: false, tools: true, streaming: true, reasoning_controls: true, model_discovery: false, token_counting: false },
	text: { vision: false, documents: false, tools: false, streaming: true, reasoning_controls: false, model_discovery: true, token_counting: false },
};

test('capability warnings name what a model cannot do, in chip and sentence form', () => {
	assert.deepEqual(capabilityWarnings(presets[0], caps.sonnet), []);
	const gpt = capabilityWarnings(presets[2], caps.gpt);
	assert.deepEqual(gpt.map((w) => w.id), ['documents']);
	assert.equal(gpt[0].chip, 'no documents');
	assert.match(gpt[0].detail, /GPT-5\.4/);
	assert.match(gpt[0].detail, /PDF|document/i);
	const text = capabilityWarnings({ id: 'text', name: 'Plain text', provider: 'openrouter', model: 'vendor/text-only' }, caps.text);
	assert.deepEqual(text.map((w) => w.id), ['vision', 'documents', 'tools']);
	assert.deepEqual(text.map((w) => w.chip), ['no vision', 'no documents', 'no tools']);
	assert.match(text[0].detail, /image|photo|screenshot/i);
	assert.match(text[2].detail, /tool/i);
	for (const w of text) assert.match(w.detail, /Plain text/);
	// A model whose name is blank is called by its id.
	assert.match(capabilityWarnings({ id: 'x', name: '  ', provider: 'openai', model: 'gpt-5.4' }, caps.gpt)[0].detail, /gpt-5\.4/);
});

test('unknown capabilities warn about nothing', () => {
	assert.deepEqual(capabilityWarnings(presets[2], undefined), []);
	assert.deepEqual(capabilityWarnings(presets[2], null), []);
	assert.deepEqual(presetCapabilities({ capabilities: caps }, 'gpt'), caps.gpt);
	assert.equal(presetCapabilities({ capabilities: caps }, 'missing'), undefined);
	assert.equal(presetCapabilities({}, 'gpt'), undefined);
	assert.equal(presetCapabilities(null, 'gpt'), undefined);
});

test('connection test outcomes read as one sentence each, typed by the server error', () => {
	const gpt = presets[2];
	const ok = presetTestCopy({ ok: true, preset: 'gpt', provider: 'openai', model: 'gpt-5.4', usage: { input_tokens: 8, output_tokens: 1 }, capabilities: caps.gpt }, gpt);
	assert.equal(ok.tone, 'ok');
	assert.match(ok.text, /gpt-5\.4/);
	assert.match(ok.text, /answered/);
	assert.match(ok.text, /9 tokens/);
	// [error, what the server says, what the person reads]
	const cases = [
		['setup_required', 'no OpenAI API key is configured', /no OpenAI API key.*API keys/i],
		['authentication', 'OpenAI rejected the API key: Incorrect API key provided', /OpenAI rejected the API key\. Change it under API keys\./],
		['rate_limited', 'OpenAI accepted the key but is rate limiting: slow down', /key works.*in a moment/],
		['model_not_found', 'OpenAI has no model "gpt-5.4": The model does not exist', /no model "gpt-5\.4"\. Check the model id\./],
		['provider_rejected', 'OpenAI rejected the request (402): Insufficient credits', /^OpenAI rejected the request \(402\): Insufficient credits$/],
		['provider_unavailable', 'OpenAI answered 503: down', /^OpenAI answered 503: down\. Try again/],
		['unreachable', 'failed to reach OpenAI: connection refused', /^failed to reach OpenAI: connection refused\. Check/],
		['timeout', 'OpenAI did not answer in time', /OpenAI did not answer in time\. Try again\./],
		['invalid_response', 'OpenAI answered with something unexpected: no choices', /^OpenAI answered with something unexpected: no choices$/],
		['unsupported', 'OpenAI does not support tools for "gpt-5.4"', /^OpenAI does not support tools/],
		['unknown_preset', 'model preset "gpt" does not exist', /Save the preset first/],
	];
	for (const [error, message, pattern] of cases) {
		const copy = presetTestCopy({ ok: false, error, message, status: 422 }, gpt);
		assert.equal(copy.tone, 'error', error);
		assert.match(copy.text, pattern, `${error}: ${copy.text}`);
	}
	// A rate limit means the key works; the copy says so and how long to wait.
	const limited = presetTestCopy({ ok: false, error: 'rate_limited', message: 'slow down', status: 429, retry_after_seconds: 7 }, gpt);
	assert.match(limited.text, /key works/i);
	assert.match(limited.text, /in 7 s/);
	// An error the client does not know still shows the server's message.
	const other = presetTestCopy({ ok: false, error: 'something_new', message: 'the server said this', status: 500 }, gpt);
	assert.equal(other.tone, 'error');
	assert.equal(other.text, 'the server said this');
});
