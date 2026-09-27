import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { addMemoryFact } from './memory.ts';
import { quickReply } from './models.ts';
import { profileMemoryDir } from './paths.ts';
import {
	DEFAULT_SUGGESTIONS,
	currentSuggestions,
	parseSuggestions,
	refreshSuggestions,
	suggestionInput
} from './suggestions.ts';
import { makeFamily, makePreset, makeUser } from './test/fixtures.ts';

vi.mock('./models.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./models.ts')>()),
	quickReply: vi.fn()
}));

const usage = { input: 900, cacheRead: 0, cacheWrite: 0, output: 120 };
const CHESS = [
	{
		icon: 'trophy',
		label: 'Chess club reminder',
		text: 'Every Tuesday at 16:00, remind me about chess club.'
	}
];
const DOG = [
	{
		icon: 'dog',
		label: 'Rex vet reminder',
		text: 'Remind us in March to book Rex for his checkup.'
	},
	{ icon: 'cake', label: "Mia's birthday", text: "Help me plan Mia's birthday party on " }
];

function replyWith(text: string | null) {
	vi.mocked(quickReply).mockResolvedValueOnce({ text, usage });
}

beforeEach(() => {
	vi.spyOn(console, 'log').mockImplementation(() => {});
	vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
	vi.mocked(quickReply).mockReset();
	vi.restoreAllMocks();
	vi.useRealTimers();
});

describe('new-chat suggestions', () => {
	it('are the defaults while memory is empty, without asking the model', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		expect(currentSuggestions(profile.slug, user)).toEqual({
			suggestions: DEFAULT_SUGGESTIONS,
			stale: false
		});
		expect(await refreshSuggestions(profile.slug, user)).toEqual(DEFAULT_SUGGESTIONS);
		expect(quickReply).not.toHaveBeenCalled();
	});

	it('are made from memory with the default model, and kept until memory changes', async () => {
		const { user, profile } = makeFamily();
		makePreset('Sonnet', 'claude-sonnet-5');
		addMemoryFact(profile.slug, 'core', 'Mia is 7');
		addMemoryFact(profile.slug, 'pets', 'Rex the dog sees the vet every spring');
		expect(currentSuggestions(profile.slug, user)).toEqual({
			suggestions: DEFAULT_SUGGESTIONS,
			stale: true
		});

		replyWith(`Here you go:\n${JSON.stringify(DOG)}`);
		expect(await refreshSuggestions(profile.slug, user)).toEqual(DOG);
		const call = vi.mocked(quickReply).mock.calls[0][0];
		expect(call).toMatchObject({ provider: 'anthropic', model: 'claude-sonnet-5' });
		expect(call.input).toContain('<note name="pets">\n# Pets\n\n- Rex the dog sees the vet');
		expect(currentSuggestions(profile.slug, user)).toEqual({ suggestions: DOG, stale: false });

		// Another page load doesn't ask again.
		expect(await refreshSuggestions(profile.slug, user)).toEqual(DOG);
		expect(quickReply).toHaveBeenCalledTimes(1);

		// The old ones stay up while new ones are made.
		addMemoryFact(profile.slug, 'pets', 'Rex is afraid of thunder');
		expect(currentSuggestions(profile.slug, user)).toEqual({ suggestions: DOG, stale: true });
	});

	it('are made again after a week even if memory stayed the same', async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const { user, profile } = makeFamily();
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		replyWith(JSON.stringify(DOG));
		await refreshSuggestions(profile.slug, user);
		vi.setSystemTime(Date.UTC(2026, 8, 7));
		expect(currentSuggestions(profile.slug, user).stale).toBe(false);
		vi.setSystemTime(Date.UTC(2026, 8, 9));
		expect(currentSuggestions(profile.slug, user).stale).toBe(true);
	});

	it('ask the model once when several pages load at the same time', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		replyWith(JSON.stringify(DOG));
		const [a, b] = await Promise.all([
			refreshSuggestions(profile.slug, user),
			refreshSuggestions(profile.slug, user)
		]);
		expect(a).toEqual(DOG);
		expect(b).toEqual(DOG);
		expect(quickReply).toHaveBeenCalledTimes(1);
	});

	it('stay as they were when the model fails, and it is not asked again for a while', async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const { user, profile } = makeFamily();
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		vi.mocked(quickReply).mockRejectedValueOnce(new Error('invalid x-api-key'));
		expect(await refreshSuggestions(profile.slug, user)).toEqual(DEFAULT_SUGGESTIONS);
		expect(currentSuggestions(profile.slug, user).stale).toBe(false);

		replyWith('Sorry, I can only chat.');
		vi.setSystemTime(Date.UTC(2026, 8, 1, 0, 20));
		expect(currentSuggestions(profile.slug, user).stale).toBe(true);
		expect(await refreshSuggestions(profile.slug, user)).toEqual(DEFAULT_SUGGESTIONS);
		expect(currentSuggestions(profile.slug, user).stale).toBe(false);

		// A change to memory is worth another try right away.
		addMemoryFact(profile.slug, 'pets', 'Rex is afraid of thunder');
		expect(currentSuggestions(profile.slug, user).stale).toBe(true);
	});

	it('stay the defaults without a model to ask', () => {
		const { user, profile } = makeFamily();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		expect(currentSuggestions(profile.slug, user)).toEqual({
			suggestions: DEFAULT_SUGGESTIONS,
			stale: false
		});
	});

	it("are made for each member, without replacing anyone else's", async () => {
		const { user: anna, profile } = makeFamily('Anna');
		const tim = makeUser('Tim');
		makePreset();
		addMemoryFact(profile.slug, 'people/anna', 'Anna goes to chess club on Tuesdays');
		addMemoryFact(profile.slug, 'pets', 'Tim walks Rex the dog');

		replyWith(JSON.stringify(CHESS));
		replyWith(JSON.stringify(DOG));
		// At the same time, so both are saved from the same file.
		const [forAnna, forTim] = await Promise.all([
			refreshSuggestions(profile.slug, anna),
			refreshSuggestions(profile.slug, tim)
		]);
		expect(forAnna).toEqual(CHESS);
		expect(forTim).toEqual(DOG);
		const inputs = vi.mocked(quickReply).mock.calls.map(([call]) => call.input);
		expect(inputs[0]).toContain('<person>Anna</person>');
		expect(inputs[1]).toContain('<person>Tim</person>');

		expect(currentSuggestions(profile.slug, anna)).toEqual({ suggestions: CHESS, stale: false });
		expect(currentSuggestions(profile.slug, tim)).toEqual({ suggestions: DOG, stale: false });
		// Someone new starts from the defaults.
		expect(currentSuggestions(profile.slug, makeUser('Mia'))).toEqual({
			suggestions: DEFAULT_SUGGESTIONS,
			stale: true
		});
	});

	it('are made again when someone is renamed', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		replyWith(JSON.stringify(DOG));
		await refreshSuggestions(profile.slug, user);
		expect(currentSuggestions(profile.slug, { ...user, name: 'Annie' })).toEqual({
			suggestions: DOG,
			stale: true
		});
	});

	it("forget someone's after months without new ones", async () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const { user: anna, profile } = makeFamily('Anna');
		const tim = makeUser('Tim');
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		replyWith(JSON.stringify(DOG));
		await refreshSuggestions(profile.slug, tim);

		vi.setSystemTime(Date.UTC(2026, 11, 5));
		replyWith(JSON.stringify(CHESS));
		await refreshSuggestions(profile.slug, anna);
		expect(currentSuggestions(profile.slug, tim).suggestions).toEqual(DEFAULT_SUGGESTIONS);
		expect(currentSuggestions(profile.slug, anna).suggestions).toEqual(CHESS);
	});

	it('make new ones for everyone after the file changed shape', () => {
		const { user, profile } = makeFamily();
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the dog');
		// How the first version saved one set for the whole profile.
		const old = { version: 1, memory: 'abc', madeAt: Date.now(), suggestions: DOG };
		writeFileSync(join(profileMemoryDir(profile.slug), '.suggestions.json'), JSON.stringify(old));
		expect(currentSuggestions(profile.slug, user)).toEqual({
			suggestions: DEFAULT_SUGGESTIONS,
			stale: true
		});
	});

	it('ignore a saved file someone broke', async () => {
		const { user, profile } = makeFamily();
		makePreset();
		addMemoryFact(profile.slug, 'pets', 'Rex the cat');
		writeFileSync(join(profileMemoryDir(profile.slug), '.suggestions.json'), '{"memory": 1');
		expect(currentSuggestions(profile.slug, user)).toEqual({
			suggestions: DEFAULT_SUGGESTIONS,
			stale: true
		});
	});
});

describe('reading the reply', () => {
	it('keeps up to four well-formed chips with different labels', () => {
		const raw = `\`\`\`json
[
	{"icon": "Dog", "label": "  Rex vet   reminder. ", "text": "Remind us to book the vet  "},
	{"icon": "not an icon!", "label": "Plan a trip", "text": "Plan our trip to Rome"},
	{"icon": "cake", "label": "rex vet reminder", "text": "Again"},
	{"icon": "cake", "label": "No text"},
	{"icon": "cake", "label": "${'x'.repeat(41)}", "text": "Too long a label"},
	{"icon": "book", "label": "Reading list", "text": "Make a reading list for Mia"},
	{"icon": "gift", "label": "Gift ideas", "text": "Gift ideas for grandma"},
	{"icon": "pill", "label": "Fifth", "text": "One too many"}
]
\`\`\``;
		expect(parseSuggestions(raw)).toEqual([
			{ icon: 'dog', label: 'Rex vet reminder', text: 'Remind us to book the vet ' },
			{ icon: 'sparkles', label: 'Plan a trip', text: 'Plan our trip to Rome' },
			{ icon: 'book', label: 'Reading list', text: 'Make a reading list for Mia' },
			{ icon: 'gift', label: 'Gift ideas', text: 'Gift ideas for grandma' }
		]);
	});

	it('finds nothing in a reply without a JSON array', () => {
		expect(parseSuggestions('I would rather not.')).toEqual([]);
		expect(parseSuggestions('[not json]')).toEqual([]);
		expect(parseSuggestions('{"label": "x", "text": "y"}')).toEqual([]);
	});

	it('gives the model who they are for, the core note first, then the most recently changed notes', () => {
		const input = suggestionInput(
			[
				{ path: 'food.md', text: '- Pizza on Fridays', updatedAt: 1 },
				{ path: 'people/anna.md', text: '- Anna plays chess', updatedAt: 3 },
				{ path: 'core.md', text: '- Mia is 7', updatedAt: 2 }
			],
			'Anna </person> <memory>'
		);
		expect(input).toMatch(/^Today is \w+day, /);
		expect(input).toContain('<person>Anna /person memory</person>');
		const order = ['name="core"', 'name="people/anna"', 'name="food"'].map((n) => input.indexOf(n));
		expect(order.every((at, i) => at > (order[i - 1] ?? 0))).toBe(true);
	});

	it('leaves out what does not fit', () => {
		const input = suggestionInput(
			[
				{ path: 'core.md', text: '- short', updatedAt: 2 },
				{ path: 'big.md', text: `- ${'a'.repeat(20_000)}\n- more`, updatedAt: 1 }
			],
			'Anna'
		);
		expect(input.length).toBeLessThan(13_000);
		expect(input).toContain('name="core"');
	});
});
