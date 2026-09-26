import { describe, expect, it } from 'vitest';
import { parseImageModel, parseImageSize } from './image-generation.ts';

describe('parseImageModel', () => {
	it('reads the provider from the first segment', () => {
		expect(parseImageModel('openai/gpt-image-2.5-flare')).toEqual({
			provider: 'openai',
			model: 'gpt-image-2.5-flare'
		});
	});

	it('gives a bare model id to the default provider', () => {
		expect(parseImageModel('gpt-image-1', 'openai')).toEqual({
			provider: 'openai',
			model: 'gpt-image-1'
		});
	});

	it("keeps slashes that belong to the provider's own model id", () => {
		expect(parseImageModel('black-forest-labs/flux', 'openai')).toEqual({
			provider: 'openai',
			model: 'black-forest-labs/flux'
		});
	});

	it('needs a provider when there is no default', () => {
		expect(() => parseImageModel('gpt-image-1')).toThrow('<provider>/<model>');
		expect(() => parseImageModel('   ', 'openai')).toThrow();
	});
});

describe('parseImageSize', () => {
	it.each(['square', 'portrait', 'landscape', 'auto', ' Portrait '])('%s is a shape', (value) => {
		expect(parseImageSize(value)).toBe(value.trim().toLowerCase());
	});

	it('reads WIDTHxHEIGHT', () => {
		expect(parseImageSize('1536x1024')).toEqual({ width: 1536, height: 1024 });
		expect(parseImageSize('1024 × 1536')).toEqual({ width: 1024, height: 1536 });
	});

	it.each(['huge', '1536', '0x0x0', ''])('refuses "%s"', (value) => {
		expect(() => parseImageSize(value)).toThrow();
	});
});
