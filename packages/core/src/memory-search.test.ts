import { describe, expect, it } from 'vitest';
import { addMemoryFact, writeMemoryNote } from './memory.ts';
import { rankFacts, recallFor, searchMemory } from './memory-search.ts';
import { makeFamily } from './test/fixtures.ts';

function fact(text: string, path = 'family.md', heading: string | null = null, line = 3) {
	return { path, line, heading, text };
}

const texts = (hits: { text: string }[]) => hits.map((hit) => hit.text);

describe('rankFacts', () => {
	it('matches other forms of a word, in any language', () => {
		const facts = [
			fact('Mia is allergic to nuts'),
			fact('Пароль от вайфая: mango42'),
			fact('Der Kinderzahnarzt ist Dr. Keller'),
			fact('Café on the corner opens at 8')
		];
		expect(texts(rankFacts(facts, 'any allergies?', { limit: 5 }))).toEqual([
			'Mia is allergic to nuts'
		]);
		expect(texts(rankFacts(facts, 'какой пароль от вайфай?', { limit: 5 }))).toEqual([
			'Пароль от вайфая: mango42'
		]);
		expect(texts(rankFacts(facts, 'Wer ist unser Zahnarzt?', { limit: 5 }))).toEqual([
			'Der Kinderzahnarzt ist Dr. Keller'
		]);
		expect(texts(rankFacts(facts, 'is the cafe open', { limit: 5 }))).toEqual([
			'Café on the corner opens at 8'
		]);
	});

	it('ignores words that say nothing, and short words match only whole', () => {
		const facts = [fact('The dog is called Rex'), fact('Tomatoes grow in the garden')];
		expect(rankFacts(facts, 'what is the thing', { limit: 5 })).toEqual([]);
		expect(texts(rankFacts(facts, 'feed the dog', { limit: 5 }))).toEqual([
			'The dog is called Rex'
		]);
		expect(rankFacts(facts, 'Tom', { limit: 5 })).toEqual([]);
	});

	it('finds facts by their note and heading, below ones that say it themselves', () => {
		const facts = [
			fact('Peanuts', 'people/anna.md', 'Allergies', 5),
			fact('Likes tea', 'people/anna.md', null, 3),
			fact('Ben is allergic to cats', 'people/ben.md')
		];
		expect(texts(rankFacts(facts, 'What is Anna allergic to?', { limit: 5 }))).toEqual([
			'Peanuts',
			'Ben is allergic to cats',
			'Likes tea'
		]);
	});

	it('puts rare words first and leaves out much weaker matches', () => {
		const facts = [
			fact('The piano lesson is on Tuesday'),
			...Array.from({ length: 6 }, (_, i) => fact(`Tuesday chore ${i}`)),
			fact('Swimming is on Friday')
		];
		const hits = rankFacts(facts, 'when is the piano lesson on tuesday', {
			limit: 5,
			cutoff: 0.5
		});
		expect(texts(hits)).toEqual(['The piano lesson is on Tuesday']);
	});

	it('lifts facts about the sender only among ones that match anyway', () => {
		const facts = [
			fact('Ben likes coffee', 'people/ben.md'),
			fact('Anna likes tea', 'people/anna.md'),
			fact('Anna was born in Riga', 'people/anna.md')
		];
		expect(texts(rankFacts(facts, 'what do I like', { limit: 5, boost: 'Anna' }))).toEqual([
			'Anna likes tea',
			'Ben likes coffee'
		]);
		expect(rankFacts(facts, 'what is the weather', { limit: 5, boost: 'Anna' })).toEqual([]);
	});
});

describe('searchMemory', () => {
	it('searches every note, the pinned one too, with the line of each fact', async () => {
		const { profile } = makeFamily();
		addMemoryFact(profile.slug, 'core', 'Mia is allergic to nuts');
		writeMemoryNote(profile.slug, 'home', '# Home\n\n## Internet\n\n- Wifi password: mango42\n');
		expect(await searchMemory(profile.slug, 'wifi')).toEqual([
			{
				path: 'home.md',
				line: 5,
				heading: 'Internet',
				text: 'Wifi password: mango42',
				score: expect.any(Number)
			}
		]);
		expect(texts(await searchMemory(profile.slug, 'allergy'))).toEqual(['Mia is allergic to nuts']);
	});
});

describe('recallFor', () => {
	it('lists the facts that match a message, by note', async () => {
		const { profile } = makeFamily();
		writeMemoryNote(profile.slug, 'home', '# Home\n\n## Internet\n\n- Wifi password: mango42\n');
		addMemoryFact(profile.slug, 'food', 'Pizza night is on Friday');
		const recall = await recallFor(profile.slug, "What's the wifi password?", { known: '' });
		expect(recall).toContain('<memory>');
		expect(recall).toContain('- [home › Internet] Wifi password: mango42');
		expect(recall).not.toContain('Pizza');
	});

	it('leaves out what the conversation already has, however it was written', async () => {
		const { profile } = makeFamily();
		addMemoryFact(profile.slug, 'core', 'Mia is allergic to **nuts**');
		addMemoryFact(profile.slug, 'kids', 'Mia goes to Riverside school');
		const known = '<note name="core">\n# Core\n\n- Mia is allergic to **nuts**\n</note>';
		const recall = await recallFor(profile.slug, 'Can Mia eat this at school?', { known });
		expect(recall).toContain('Mia goes to Riverside school');
		expect(recall).not.toContain('allergic');
		expect(
			await recallFor(profile.slug, 'Can Mia eat this at school?', {
				known: `${known}\nMIA goes to  riverside School.`
			})
		).toBeNull();
	});

	it('is null without memory, words or matches', async () => {
		const { profile } = makeFamily();
		expect(await recallFor(profile.slug, 'wifi?', { known: '' })).toBeNull();
		addMemoryFact(profile.slug, 'home', 'Wifi password: mango42');
		expect(await recallFor(profile.slug, '   ', { known: '' })).toBeNull();
		expect(await recallFor(profile.slug, 'thanks!', { known: '' })).toBeNull();
	});
});
