import test from 'node:test';
import assert from 'node:assert/strict';
import {
	PROVIDERS,
	effectivePresetId,
	modelShortLabel,
	presetLabel,
	presetsByProvider,
	suggestPresetId,
	validatePresets,
} from '../src/lib/models/presets.js';

const presets = [
	{ id: 'sonnet', name: 'Claude Sonnet', provider: 'anthropic', model: 'claude-sonnet-4-6' },
	{ id: 'haiku', name: 'Claude Haiku', provider: 'anthropic', model: 'claude-haiku-4-5-20251001' },
	{ id: 'gpt', name: 'GPT-5.4', provider: 'openai', model: 'gpt-5.4' },
];
const slots = { chat_preset: 'sonnet', background_preset: 'haiku' };

test('providers are the two adapters the server ships (#156)', () => {
	assert.deepEqual(PROVIDERS.map((p) => p.id), ['anthropic', 'openai']);
	for (const p of PROVIDERS) assert.ok(p.label);
});

test('valid presets and slots produce no errors', () => {
	assert.deepEqual(validatePresets(presets, slots, ['anthropic', 'openai']), []);
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
});

test('ids are derived from names and stay unique', () => {
	assert.equal(suggestPresetId('Claude Sonnet', []), 'claude-sonnet');
	assert.equal(suggestPresetId('Claude Sonnet', ['claude-sonnet']), 'claude-sonnet-2');
	assert.equal(suggestPresetId('  GPT 5.4 mini!! ', ['gpt-5-4-mini', 'gpt-5-4-mini-2']), 'gpt-5-4-mini-3');
	assert.equal(suggestPresetId('', []), 'preset');
});

test('presets group by provider in provider order', () => {
	const grouped = presetsByProvider(presets);
	assert.deepEqual(grouped.map((g) => g.provider.id), ['anthropic', 'openai']);
	assert.deepEqual(grouped[0].presets.map((p) => p.id), ['sonnet', 'haiku']);
	assert.deepEqual(presetsByProvider([]), []);
});
