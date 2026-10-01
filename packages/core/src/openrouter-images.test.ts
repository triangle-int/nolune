import { describe, expect, it } from 'vitest';
import type { ImageRequest } from './image-generation.ts';
import { fitImageRequest, type ImageModel } from './openrouter-images.ts';

const flux: ImageModel = {
	id: 'black-forest-labs/flux.2-klein-4b',
	architecture: { input_modalities: ['text', 'image'] },
	supported_parameters: {
		aspect_ratio: { type: 'enum', values: ['1:1', '4:3', '3:4', '16:9', '9:16', 'auto'] },
		output_format: { type: 'enum', values: ['png', 'jpeg'] },
		n: { type: 'range', min: 1, max: 1 },
		input_references: { type: 'range', min: 0, max: 4 }
	}
};
const flare: ImageModel = {
	id: 'openai/gpt-image-2.5-flare',
	supported_parameters: {
		aspect_ratio: { type: 'enum', values: ['1:1', '3:2', '2:3', 'auto'] },
		quality: { type: 'enum', values: ['auto', 'low', 'medium', 'high'] },
		background: { type: 'enum', values: ['auto', 'transparent', 'opaque'] },
		n: { type: 'range', min: 1, max: 10 },
		input_references: { type: 'range', min: 0, max: 16 }
	}
};
const wordsOnly: ImageModel = {
	id: 'someone/words-only',
	architecture: { input_modalities: ['text'] },
	supported_parameters: {}
};

function ask(overrides: Partial<ImageRequest> = {}): ImageRequest {
	return {
		model: 'x',
		prompt: 'a paper boat',
		images: [],
		size: 'auto',
		count: 1,
		signal: new AbortController().signal,
		...overrides
	};
}

const picture = { name: 'boat.png', data: Buffer.from('png'), mediaType: 'image/png' as const };

describe("fitting a picture request to the model's parameters", () => {
	it('asks for the nearest shape the model makes', () => {
		expect(fitImageRequest(ask({ size: 'portrait' }), flux).body.aspect_ratio).toBe('3:4');
		expect(fitImageRequest(ask({ size: 'portrait' }), flare).body.aspect_ratio).toBe('2:3');
		expect(fitImageRequest(ask({ size: 'landscape' }), flux).body.aspect_ratio).toBe('4:3');
		expect(fitImageRequest(ask({ size: 'square' }), flux).body.aspect_ratio).toBe('1:1');
		expect(fitImageRequest(ask(), flux).body.aspect_ratio).toBe('auto');
		expect(fitImageRequest(ask({ size: 'portrait' }), wordsOnly).body).not.toHaveProperty(
			'aspect_ratio'
		);
		expect(fitImageRequest(ask({ size: { width: 1536, height: 1024 } }), flux).body.size).toBe(
			'1536x1024'
		);
	});

	it('makes as many requests as the model’s n needs', () => {
		expect(fitImageRequest(ask({ count: 3 }), flux).counts).toEqual([1, 1, 1]);
		expect(fitImageRequest(ask({ count: 3 }), flare).counts).toEqual([3]);
	});

	it('sends pictures to start from as data URLs, as many as the model takes', () => {
		const { body } = fitImageRequest(ask({ images: [picture] }), flux);
		expect(body.input_references).toEqual([
			{
				type: 'image_url',
				image_url: { url: `data:image/png;base64,${picture.data.toString('base64')}` }
			}
		]);
		expect(() => fitImageRequest(ask({ images: Array(5).fill(picture) }), flux)).toThrow(
			'takes at most 4 input images'
		);
		expect(() => fitImageRequest(ask({ images: [picture] }), wordsOnly)).toThrow(
			'makes pictures from words only'
		);
	});

	it('says no to what the model can’t do, and leaves out what it ignores', () => {
		expect(() => fitImageRequest(ask({ quality: 'high' }), flux)).toThrow('has no quality setting');
		expect(fitImageRequest(ask({ quality: 'high' }), flare).body.quality).toBe('high');
		expect(() => fitImageRequest(ask({ background: 'transparent' }), flux)).toThrow(
			"can't make a transparent background"
		);
		expect(fitImageRequest(ask({ background: 'opaque' }), flux).body).not.toHaveProperty(
			'background'
		);
		expect(fitImageRequest(ask({ format: 'jpeg' }), flux).body.output_format).toBe('jpeg');
		expect(fitImageRequest(ask({ format: 'webp' }), flux).body).not.toHaveProperty('output_format');
	});
});
