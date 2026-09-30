import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { appendRow, commitQueuedRows, createConversation, insertQueued } from './conversations.ts';
import {
	addMemoryFact,
	listMemoryFiles,
	readMemoryNote,
	renameMemoryNote,
	writeMemoryNote
} from './memory.ts';
import {
	addToCard,
	bringToCard,
	cardCandidates,
	cardChanges,
	cardOf,
	cardProfiles,
	checkCardWrite,
	keepOnlyInProfile,
	profileCard,
	profileCards,
	readCard,
	recordAgentCardChanges
} from './memory-cards.ts';
import {
	memoryLooks,
	recentMemoryChanges,
	undoMemoryChange,
	MemoryUndoError
} from './memory-changes.ts';
import { importMemoryExport } from './memory-import.ts';
import { membersWithNotes } from './memory-people.ts';
import { learnFrom } from './memory-learning.ts';
import { recallFor, searchMemory } from './memory-search.ts';
import { quickReply } from './models.ts';
import { CARDS, paths } from './paths.ts';
import { addMember, createProfile, type Profile } from './profiles.ts';
import { buildSystemPrompt } from './prompt.ts';
import { makePreset, makeUser } from './test/fixtures.ts';
import { deleteUser } from './users.ts';

vi.mock('./models.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./models.ts')>()),
	quickReply: vi.fn()
}));

vi.mock('./runner.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./runner.ts')>()),
	onLoopEnd: vi.fn(),
	onRunningChange: vi.fn()
}));

afterEach(() => {
	vi.useRealTimers();
	vi.resetAllMocks();
});

type Person = { id: string; name: string };

/** Anna in three profiles: her own, the family's with Ben, and one with Zoe. */
function circles() {
	const anna = makeUser('Anna Smith');
	const ben = makeUser('Ben');
	const zoe = makeUser('Zoe');
	const own = createProfile('Anna', anna.id);
	const family = createProfile('Family', anna.id);
	addMember(family.id, 'ben@example.com');
	const friends = createProfile("Zoe's party", zoe.id);
	addMember(friends.id, 'anna smith@example.com');
	return { anna, ben, zoe, own, family, friends };
}

function chat(profile: Profile, person: Person) {
	return createConversation({ profile, presetId: makePreset().id, userId: person.id });
}

function say(conversationId: string, person: Person, text: string) {
	insertQueued({ conversationId, senderId: person.id, senderName: person.name, text });
	commitQueuedRows(conversationId);
}

function reply(conversationId: string, text: string) {
	appendRow({
		conversationId,
		role: 'assistant',
		kind: 'assistant',
		blocks: [{ type: 'text', text }]
	});
}

describe('a card', () => {
	it('is named once, by the first name when it is free', () => {
		const anna = makeUser('Anna Smith');
		const other = makeUser('Anna Lee');
		const third = makeUser('Anna');
		expect(cardOf(anna.id)).toMatchObject({ name: 'anna', path: 'cards/anna.md' });
		expect(cardOf(other.id)).toMatchObject({ name: 'anna-lee', owner: 'Anna Lee' });
		expect(cardOf(third.id).name).toBe('anna-2');
		expect(cardOf(anna.id).name).toBe('anna');
	});

	it('goes with its owner into every profile they are in, and no other', () => {
		const { anna, ben, own, family, friends } = circles();
		addToCard(cardOf(anna.id), 'Vegetarian');
		addToCard(cardOf(anna.id), 'Speaks English and German');

		for (const profile of [own, family, friends]) {
			expect(profileCard(profile, 'cards/anna').userId).toBe(anna.id);
			expect(buildSystemPrompt(profile)).toContain(
				'<card name="cards/anna" of="Anna Smith">\n# Anna Smith\n\n- Vegetarian\n- Speaks English and German\n</card>'
			);
		}
		expect(profileCards(family.id).map((c) => c.path)).toEqual(['cards/anna.md', 'cards/ben.md']);
		expect(buildSystemPrompt(family)).toContain(
			'<card name="cards/ben" of="Ben">\n(empty so far)\n</card>'
		);
		expect(buildSystemPrompt(own)).not.toContain('cards/ben');
		expect(() => profileCard(own, 'cards/ben')).toThrow(
			'"cards/ben" isn\'t the card of anyone in this profile. Its members\' cards: cards/anna.'
		);
		expect(cardProfiles(anna.id).map((p) => p.name)).toEqual(['Anna', 'Family', "Zoe's party"]);
		expect(cardProfiles(ben.id).map((p) => p.name)).toEqual(['Family']);
	});

	it('holds at most 2,000 characters, and keeps its name', () => {
		const anna = makeUser('Anna');
		const card = cardOf(anna.id);
		expect(() => addToCard(card, 'x'.repeat(2_000))).toThrow(/a card can have at most 2000/);
		expect(() => renameMemoryNote(CARDS, card.path, 'cards/someone')).toThrow(
			'A card keeps its name'
		);
		expect(() => writeMemoryNote(CARDS, 'people/anna', 'x')).toThrow("isn't a card");
		expect(() => writeMemoryNote('anna', 'cards/anna', 'x')).toThrow('is a card');
	});

	it('is found by search and recall along with the profile, and goes to the trash with its owner', async () => {
		const { anna, family } = circles();
		addToCard(cardOf(anna.id), 'Allergic to peanuts');
		const cards = profileCards(family.id).map((c) => c.path);

		expect((await searchMemory(family.slug, 'allergies', 20, cards)).map((h) => h.path)).toEqual([
			'cards/anna.md'
		]);
		expect(await searchMemory(family.slug, 'allergies')).toEqual([]);
		expect(
			await recallFor(family.slug, 'Any allergies to think of?', { known: '', cards })
		).toContain('- [cards/anna] Allergic to peanuts');
		// What the prompt has already doesn't come again.
		const known = buildSystemPrompt(family);
		expect(await recallFor(family.slug, 'Any allergies?', { known, cards })).toBeNull();

		deleteUser('Anna Smith');
		expect(existsSync(join(paths.cards, 'anna.md'))).toBe(false);
		expect(readdirSync(paths.trash).some((name) => name.startsWith('card-anna-'))).toBe(true);
	});
});

describe('writing on a card', () => {
	it("takes only its owner's own messages, in the turn the agent is answering", () => {
		const { anna, ben, family, own } = circles();
		const card = cardOf(anna.id);
		const conv = chat(family, anna);
		const chatOf = (conversationId?: string) => ({ profileId: family.id, conversationId });

		expect(() => checkCardWrite(card, chatOf())).toThrow('only what Anna Smith says');
		say(conv.id, anna, "I'm vegetarian");
		expect(() => checkCardWrite(card, chatOf(conv.id))).not.toThrow();
		reply(conv.id, 'Noted!');
		say(conv.id, ben, 'Anna also hates olives');
		expect(() => checkCardWrite(card, chatOf(conv.id))).toThrow('their note in people/');
		expect(() => checkCardWrite(cardOf(ben.id), chatOf(conv.id))).not.toThrow();
		// A chat of another profile isn't this one's.
		expect(() => checkCardWrite(card, { profileId: own.id, conversationId: conv.id })).toThrow();
	});

	it("shows the agent's changes to its owner only, with Keep only here", () => {
		const { anna, family, friends } = circles();
		const card = cardOf(anna.id);
		const conv = chat(friends, anna);
		say(conv.id, anna, 'Keep it a secret: I am planning to propose');
		const added = addToCard(card, 'Planning to propose');
		recordAgentCardChanges({ profileId: friends.id, conversationId: conv.id }, [
			{ op: 'add', note: added.path, line: added.line }
		]);

		// Not in the chat's saved memories (it shows as the command), nor on either Memory page.
		expect(memoryLooks(conv.id)).toEqual([]);
		expect(recentMemoryChanges(friends.id, { since: new Date(0), limit: 10 })).toEqual([]);
		expect(recentMemoryChanges(family.id, { since: new Date(0), limit: 10 })).toEqual([]);
		const [change] = cardChanges(card);
		expect(change).toMatchObject({
			source: 'agent',
			fact: 'Planning to propose',
			profile: { name: "Zoe's party" },
			conversation: { id: conv.id }
		});

		expect(() => keepOnlyInProfile(change.id, makeUser('Eve').id)).toThrow(
			'not a change to your card'
		);
		const kept = keepOnlyInProfile(change.id, anna.id);
		expect(kept).toMatchObject({ profile: { name: "Zoe's party" }, note: 'people/anna.md' });
		expect(readCard(card)?.text).not.toContain('propose');
		expect(readMemoryNote(friends.slug, 'people/anna').text).toContain('- Planning to propose');
		expect(buildSystemPrompt(family)).not.toContain('propose');
		expect(cardChanges(card)[0].undone).toMatchObject({ by: 'Anna Smith' });
	});
});

describe('the note-taker', () => {
	const usage = { input: 100, cacheRead: 0, cacheWrite: 0, output: 20 };
	const answer = (text: string) => vi.mocked(quickReply).mockResolvedValueOnce({ text, usage });

	it("changes a card only from its owner's messages, and only they undo it", async () => {
		const { anna, ben, family, own } = circles();
		const conv = chat(family, ben);
		say(conv.id, ben, 'Anna is allergic to cats');
		reply(conv.id, 'Noted.');
		answer('[{"op": "add", "note": "cards/anna", "fact": "Allergic to cats"}]');
		await learnFrom(conv.id);
		expect(readCard(cardOf(anna.id))).toBeNull();

		say(conv.id, anna, "I'm vegetarian, and I speak German");
		reply(conv.id, 'Got it.');
		answer(
			'[{"op": "add", "note": "cards/anna", "under": "Food", "fact": "Vegetarian"}, {"op": "add", "note": "people/anna", "fact": "Makes the family\'s Sunday dinner"}]'
		);
		await learnFrom(conv.id);
		expect(readCard(cardOf(anna.id))?.text).toBe('# Anna Smith\n\n## Food\n\n- Vegetarian\n');
		expect(buildSystemPrompt(own)).toContain('- Vegetarian');
		expect(readMemoryNote(family.slug, 'people/anna').text).toContain('Sunday dinner');

		const [look] = memoryLooks(conv.id);
		const onCard = look.changes.find((c) => c.note === 'cards/anna.md')!;
		expect(onCard.card).toEqual({ ownerId: anna.id, owner: 'Anna Smith' });
		expect(() => undoMemoryChange(family, onCard.id, 'Ben', ben.id)).toThrow(MemoryUndoError);
		expect(() => undoMemoryChange(family, onCard.id, 'Ben', ben.id)).toThrow(
			expect.objectContaining({ reason: 'owner' })
		);
		undoMemoryChange(family, onCard.id, 'Anna Smith', anna.id);
		expect(readCard(cardOf(anna.id))?.text).not.toContain('Vegetarian');
	});
});

describe('starting a card', () => {
	it('offers what her notes say, checking what several do, and moves what she picks', () => {
		vi.useFakeTimers();
		const { anna, own, family, friends } = circles();
		vi.setSystemTime(Date.UTC(2026, 0, 1));
		// As every page and prompt does: each member gets their note.
		for (const profile of [own, family, friends]) membersWithNotes(profile);
		addMemoryFact(own.slug, 'people/anna', 'Vegetarian');
		vi.setSystemTime(Date.UTC(2026, 5, 1));
		addMemoryFact(family.slug, 'people/anna', 'Vegetarian');
		addMemoryFact(family.slug, 'people/anna', "Who: Ben's sister");
		addMemoryFact(family.slug, 'people/anna', 'Makes Sunday dinner');
		addMemoryFact(family.slug, 'core', 'Anna Smith: keep answers short');
		addMemoryFact(family.slug, 'core', 'Ben: no emojis');

		const offered = cardCandidates(anna.id);
		expect(offered.map((c) => [c.profile.name, c.note, c.text, c.suggested])).toEqual([
			['Anna', 'people/anna.md', 'Vegetarian', true],
			['Family', 'people/anna.md', 'Vegetarian', true],
			['Family', 'people/anna.md', 'Makes Sunday dinner', false],
			['Family', 'core.md', 'Keep answers short', true]
		]);

		const picked = offered.filter((c) => c.suggested).map((c) => c.id);
		expect(bringToCard(anna.id, picked)).toEqual({ added: 2, left: 0 });
		const card = cardOf(anna.id);
		expect(readCard(card)?.text).toBe(
			'# Anna Smith\n\n## About\n\n- Vegetarian\n\n## Instructions\n\n- Keep answers short\n'
		);
		const [file] = listMemoryFiles(CARDS);
		expect(file.facts.find((f) => f.text === 'Vegetarian')?.learnedAt).toBe(Date.UTC(2026, 0, 1));
		expect(readMemoryNote(family.slug, 'people/anna').text).toBe(
			"# Anna\n\n- Who: Ben's sister\n- Makes Sunday dinner\n"
		);
		expect(readMemoryNote(family.slug, 'core').text).not.toContain('keep answers short');
		expect(readMemoryNote(family.slug, 'core').text).toContain('Ben: no emojis');
		expect(cardCandidates(anna.id).map((c) => c.text)).toEqual(['Makes Sunday dinner']);
	});

	it('takes the welcome import onto the card while it has room', () => {
		const { anna, family } = circles();
		const card = cardOf(anna.id);
		const result = importMemoryExport(
			family.slug,
			'Anna Smith',
			[
				{ section: 'instructions', text: 'Keep answers short', date: '2025-03-01' },
				{ section: 'identity', text: 'Lives in Berlin', date: null },
				{ section: 'career', text: 'Works as a nurse', date: null },
				{ section: 'preferences', text: 'Likes jazz', date: null },
				{ section: 'projects', text: 'Garden: planting tomatoes', date: null }
			],
			'people/anna.md',
			card
		);
		expect(result.notes.map((n) => n.path)).toEqual([
			'cards/anna.md',
			'people/anna.md',
			'projects/garden.md'
		]);
		expect(readCard(card)?.text).toBe(
			'# Anna Smith\n\n## Instructions\n\n- Keep answers short\n\n## Identity\n\n- Lives in Berlin\n\n## Preferences\n\n- Likes jazz\n'
		);
		expect(readMemoryNote(family.slug, 'people/anna').text).toContain('Works as a nurse');
	});
});
