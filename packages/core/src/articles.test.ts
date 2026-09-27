import { describe, expect, it } from 'vitest';
import { articleBefore, indefiniteArticle } from './articles.ts';

describe('indefiniteArticle', () => {
	it('goes by the sound, for the words templates meet', () => {
		const an = ['embroidered', 'origami', 'art deco', 'ink', 'umbrella', '8-bit', '80s', 'hour'];
		const a = ['watercolor', 'unicorn', 'useful', 'European', 'one-line', "'90s prom", '1970s'];
		for (const w of an) expect(indefiniteArticle(w), w).toBe('an');
		for (const w of a) expect(indefiniteArticle(w), w).toBe('a');
	});
});

describe('articleBefore', () => {
	it('changes only a trailing "a " before a word that wants "an"', () => {
		expect(articleBefore('Create a ', 'Embroidered')).toBe('Create an ');
		expect(articleBefore('A ', 'origami')).toBe('An ');
		expect(articleBefore('Create a ', 'Retro')).toBe('Create a ');
		expect(articleBefore('into a ', '')).toBe('into a ');
		expect(articleBefore('Make it ', 'origami')).toBe('Make it ');
		expect(articleBefore('Visit Florida ', 'origami')).toBe('Visit Florida ');
	});
});
