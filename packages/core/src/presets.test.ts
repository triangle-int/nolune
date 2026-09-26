import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fetchContextWindow } from './anthropic.ts';
import {
	addPreset,
	effectiveContextWindow,
	getDefaultPreset,
	listPresets,
	removePreset,
	setDefaultPreset
} from './presets.ts';
import { makePreset } from './test/fixtures.ts';

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
