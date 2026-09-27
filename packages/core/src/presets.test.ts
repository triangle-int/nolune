import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fetchContextWindow } from './anthropic.ts';
import { createConversation, getConversation } from './conversations.ts';
import {
	addPreset,
	editPreset,
	effectiveContextWindow,
	getDefaultPreset,
	listPresets,
	removePreset,
	setDefaultPreset
} from './presets.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	fetchContextWindow: vi.fn()
}));

beforeEach(() => {
	vi.mocked(fetchContextWindow).mockReset().mockResolvedValue(200_000);
});

describe('addPreset', () => {
	it("records the model's context window", async () => {
		const preset = await addPreset({ model: ' claude-opus-5-5 ' });
		expect(fetchContextWindow).toHaveBeenCalledWith('claude-opus-5-5');
		expect(preset).toMatchObject({
			name: 'claude-opus-5-5 (anthropic)',
			model: 'claude-opus-5-5',
			contextWindow: null,
			modelContextWindow: 200_000,
			isDefault: false
		});
		expect(listPresets()).toEqual([preset]);
	});

	it('refuses a taken name, a bad context window and a model the API does not know', async () => {
		await addPreset({ model: 'claude-sonnet-5', name: 'Everyday' });
		await expect(addPreset({ model: 'claude-opus-5-5', name: 'EVERYDAY' })).rejects.toThrow(
			'A preset named "EVERYDAY" already exists'
		);
		await expect(addPreset({ model: 'claude-opus-5-5', contextWindow: 0 })).rejects.toThrow(
			'Context window must be a positive number'
		);
		vi.mocked(fetchContextWindow).mockRejectedValue(new Error('not_found_error'));
		await expect(addPreset({ model: 'claude-nope' })).rejects.toThrow(
			'Could not verify model "claude-nope"'
		);
		expect(listPresets()).toHaveLength(1);
	});
});

describe('editPreset', () => {
	it('renames and sets the window without asking the provider', async () => {
		const preset = makePreset('Sonnet');
		makePreset('Opus', 'claude-opus-5-5');
		const edited = await editPreset('Sonnet', { name: ' Everyday ', contextWindow: 1_000_000 });
		expect(edited).toMatchObject({
			id: preset.id,
			name: 'Everyday',
			model: 'claude-sonnet-5',
			contextWindow: 1_000_000,
			modelContextWindow: 200_000
		});
		expect(listPresets()[0]).toEqual(edited);
		expect(fetchContextWindow).not.toHaveBeenCalled();

		// Back to the model's own window; what's left out stays.
		expect(await editPreset(preset.id, { contextWindow: null })).toMatchObject({
			name: 'Everyday',
			contextWindow: null
		});
	});

	it('checks a new model, and a default name follows it', async () => {
		const preset = makePreset('claude-sonnet-5 (anthropic)');
		vi.mocked(fetchContextWindow).mockResolvedValue(1_000_000);
		expect(await editPreset(preset.id, { model: 'claude-opus-5-5' })).toMatchObject({
			name: 'claude-opus-5-5 (anthropic)',
			model: 'claude-opus-5-5',
			modelContextWindow: 1_000_000
		});
		expect(fetchContextWindow).toHaveBeenCalledWith('claude-opus-5-5');

		// A name of its own stays; an empty one is the default again.
		await editPreset(preset.id, { name: 'Smart' });
		expect(await editPreset(preset.id, { model: 'claude-fable-5-1' })).toMatchObject({
			name: 'Smart'
		});
		expect(await editPreset(preset.id, { name: '' })).toMatchObject({
			name: 'claude-fable-5-1 (anthropic)'
		});
	});

	it('refuses a taken name, a bad window, an unknown model or preset, and changes nothing', async () => {
		const sonnet = makePreset('Sonnet');
		makePreset('Opus', 'claude-opus-5-5');
		await expect(editPreset('Sonnet', { name: 'OPUS' })).rejects.toThrow(
			'A preset named "OPUS" already exists'
		);
		await expect(editPreset('Sonnet', { contextWindow: -1 })).rejects.toThrow(
			'Context window must be a positive number'
		);
		await expect(editPreset('Sonnet', { model: ' ' })).rejects.toThrow('Model is required');
		await expect(editPreset('Sonnet', { provider: 'mistral' })).rejects.toThrow(
			'Unsupported provider "mistral"'
		);
		vi.mocked(fetchContextWindow).mockRejectedValue(new Error('not_found_error'));
		await expect(editPreset('Sonnet', { model: 'claude-nope' })).rejects.toThrow(
			'Could not verify model "claude-nope"'
		);
		await expect(editPreset('Haiku', { name: 'Fast' })).rejects.toThrow('No preset "Haiku"');
		// Its own name, in other letters, is fine.
		expect(await editPreset('Sonnet', { name: 'SONNET' })).toMatchObject({ name: 'SONNET' });
		expect(listPresets()[0]).toMatchObject({ id: sonnet.id, model: 'claude-sonnet-5' });
	});

	it('leaves chats already on it alone, and new ones get the change', async () => {
		const { user, profile } = makeFamily();
		const preset = makePreset('Everyday');
		const before = createConversation({ profile, presetId: preset.id, userId: user.id });

		await editPreset(preset.id, { model: 'claude-opus-5-5', name: 'Smart' });
		expect(getConversation(before.id)).toMatchObject({
			presetId: preset.id,
			presetName: 'Everyday',
			model: 'claude-sonnet-5'
		});
		const after = createConversation({ profile, presetId: preset.id, userId: user.id });
		expect(after).toMatchObject({ presetName: 'Smart', model: 'claude-opus-5-5' });
	});
});

describe('default preset', () => {
	it('is the oldest until one is picked, and only one at a time', () => {
		expect(getDefaultPreset()).toBeUndefined();
		const sonnet = makePreset('Sonnet');
		const opus = makePreset('Opus');
		expect(getDefaultPreset()?.id).toBe(sonnet.id);

		setDefaultPreset('Opus');
		expect(getDefaultPreset()?.id).toBe(opus.id);
		setDefaultPreset(sonnet.id);
		expect(listPresets().filter((p) => p.isDefault)).toEqual([{ ...sonnet, isDefault: true }]);
		expect(() => setDefaultPreset('Haiku')).toThrow('No preset "Haiku"');
	});

	it('falls back to the oldest when the default is removed', () => {
		const sonnet = makePreset('Sonnet');
		makePreset('Opus');
		setDefaultPreset('Opus');
		removePreset('Opus');
		expect(getDefaultPreset()?.id).toBe(sonnet.id);
		expect(() => removePreset('Opus')).toThrow('No preset "Opus"');
	});
});

it("prefers the admin's context window over the model's", () => {
	const preset = makePreset();
	expect(effectiveContextWindow(preset)).toBe(200_000);
	expect(effectiveContextWindow({ ...preset, contextWindow: 1_000_000 })).toBe(1_000_000);
});
