import { describe, expect, it } from 'vitest';
import { createConversation } from './conversations.ts';
import { addMemoryFact, readMemoryNote, writeMemoryNote } from './memory.ts';
import { recentMemoryChanges, recordMemoryChanges, undoMemoryChange } from './memory-changes.ts';
import {
	addMemberWithNote,
	linkPersonNote,
	membersWithNotes,
	memberWords,
	mergeProfileNotes,
	moveProfileNote,
	peopleGuide,
	personNoteCandidates
} from './memory-people.ts';
import { searchMemory } from './memory-search.ts';
import { addMember, isMember } from './profiles.ts';
import { buildSystemPrompt } from './prompt.ts';
import { makeFamily, makePreset, makeUser } from './test/fixtures.ts';

const olga = "# Olga\n\n- Who: Anna's grandmother\n- Also called: grandma, бабушка\n";

describe('the notes that may be about someone', () => {
	it('are found by their name, title and what they are called, in any alphabet', () => {
		const { profile } = makeFamily();
		writeMemoryNote(profile.slug, 'people/olga', olga);
		writeMemoryNote(profile.slug, 'people/yulia', '# Yulia\n\n- The nanny\n');
		writeMemoryNote(profile.slug, 'people/max-petrov', '# Max Petrov\n\n- Anna’s colleague\n');
		writeMemoryNote(profile.slug, 'people/babushka', '# Nina\n\n- Also called: Granny Nina\n');
		const paths = (name: string) =>
			personNoteCandidates(profile.slug, name).map((note) => note.path);

		expect(paths('Ольга Петрова')).toEqual(['people/olga.md']);
		expect(paths('Юля')).toEqual(['people/yulia.md']);
		expect(paths('Max Petrov')).toEqual(['people/max-petrov.md']);
		expect(paths('Max')).toEqual(['people/max-petrov.md']);
		expect(paths('Nina Ivanova')).toEqual(['people/babushka.md']);
		expect(paths('Leo')).toEqual([]);
		expect(personNoteCandidates(profile.slug, 'Olga')[0]).toEqual({
			path: 'people/olga.md',
			title: 'Olga',
			who: "Anna's grandmother",
			aliases: ['grandma', 'бабушка'],
			facts: []
		});
	});
});

describe('adding a member', () => {
	it('asks first whether a note memory has is about them', () => {
		const { profile } = makeFamily();
		const olgaUser = makeUser('Olga');
		writeMemoryNote(profile.slug, 'people/olga', `${olga}- Loves roses\n`);

		const asked = addMemberWithNote(profile, 'Olga');
		expect(asked).toMatchObject({ added: false, name: 'Olga' });
		expect(asked.added === false && asked.candidates.map((c) => c.facts)).toEqual([
			['Loves roses']
		]);
		expect(isMember(profile.id, olgaUser.id)).toBe(false);

		expect(addMemberWithNote(profile, 'Olga', 'people/olga')).toEqual({
			added: true,
			name: 'Olga',
			note: 'people/olga.md'
		});
		expect(membersWithNotes(profile).find((m) => m.name === 'Olga')).toEqual({
			id: olgaUser.id,
			name: 'Olga',
			note: 'people/olga.md',
			exists: true,
			candidates: []
		});
	});

	it('starts a note of their own when it is someone else, or nothing is about them', () => {
		const { profile } = makeFamily();
		makeUser('Olga');
		makeUser('Leo');
		writeMemoryNote(profile.slug, 'people/olga', olga);
		expect(addMemberWithNote(profile, 'Olga', 'new')).toMatchObject({
			note: 'people/olga-2.md'
		});
		expect(addMemberWithNote(profile, 'leo@example.com')).toMatchObject({
			added: true,
			note: 'people/leo.md'
		});
		expect(() => addMemberWithNote(profile, 'Leo')).toThrow('Leo is already a member');
		expect(() => addMemberWithNote(profile, 'Nobody')).toThrow('No user "Nobody"');
	});

	it("never takes another member's note, or one outside people/", () => {
		const { profile, user } = makeFamily();
		makeUser('Olga');
		writeMemoryNote(profile.slug, 'people/anna', '# Anna\n\n- Likes tea\n');
		linkPersonNote(profile, user.id, 'people/anna');
		expect(() => addMemberWithNote(profile, 'Olga', 'people/anna')).toThrow(
			"people/anna.md is Anna's note."
		);
		expect(() => addMemberWithNote(profile, 'Olga', 'home')).toThrow("isn't a person's note");
		expect(() => addMemberWithNote(profile, 'Olga', 'people/../core')).toThrow(
			"isn't a person's note"
		);
	});
});

describe("members' notes", () => {
	it("keep one for a member nothing is about yet, and wait for someone to say which it is when it's unclear", () => {
		const { profile, user } = makeFamily();
		const max = makeUser('Max');
		writeMemoryNote(profile.slug, 'people/max', '# Max\n\n- Plays chess\n');
		addMember(profile.id, 'Max');

		const [anna, maxNote] = membersWithNotes(profile);
		expect(anna).toEqual({
			id: user.id,
			name: 'Anna',
			note: 'people/anna.md',
			exists: false,
			candidates: []
		});
		expect(maxNote).toMatchObject({ note: null, exists: false });
		expect(maxNote.candidates.map((c) => c.path)).toEqual(['people/max.md']);
		expect(peopleGuide(profile)).toBe(
			'- Anna: card cards/anna, note people/anna (nothing in it yet)\n- Max: card cards/max, no note here linked yet'
		);

		expect(linkPersonNote(profile, max.id, 'people/max')).toBe('people/max.md');
		addMemoryFact(profile.slug, 'people/anna', 'Likes tea');
		expect(peopleGuide(profile)).toBe(
			'- Anna: card cards/anna, note people/anna\n- Max: card cards/max, note people/max'
		);
		expect(buildSystemPrompt(profile)).toContain(
			'The members of this profile when this conversation started, each with their card and their note here. What someone says about themselves ("I", "my") goes on their card or in their note here, as Cards below says:\n- Anna: card cards/anna, note people/anna\n- Max: card cards/max, note people/max\n'
		);
	});

	it('go along when a note is moved or merged, with what the note-taker saved in them', () => {
		const { profile, user } = makeFamily();
		const olgaUser = makeUser('Olga');
		addMemberWithNote(profile, 'Olga');
		const conv = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		const added = addMemoryFact(profile.slug, 'people/grandma', 'Loves roses');
		recordMemoryChanges({
			profileId: profile.id,
			conversationId: conv.id,
			afterMessageId: 1,
			changes: [{ op: 'add', note: added.path, line: added.line, createdNote: true }]
		});
		const note = () => membersWithNotes(profile).find((m) => m.id === olgaUser.id)?.note;
		expect(note()).toBe('people/olga.md');
		addMemoryFact(profile.slug, 'people/olga', 'Lives in Tver');

		// Two notes about one person: people/grandma goes into hers.
		expect(mergeProfileNotes(profile, 'people/grandma', 'people/olga')).toMatchObject({
			into: 'people/olga.md'
		});
		expect(readMemoryNote(profile.slug, 'people/olga').text).toBe(
			'# Olga\n\n- Lives in Tver\n- Loves roses\n- Also called: Grandma\n'
		);
		const [change] = recentMemoryChanges(profile.id, { since: new Date(0), limit: 5 });
		expect(change.note).toBe('people/olga.md');

		expect(moveProfileNote(profile, 'people/olga', 'people/olga-petrova')).toEqual({
			from: 'people/olga.md',
			to: 'people/olga-petrova.md'
		});
		expect(note()).toBe('people/olga-petrova.md');
		// Undo finds the fact where it is now, and leaves the note, which has more in it.
		undoMemoryChange(profile, change.id, 'Anna');
		expect(readMemoryNote(profile.slug, 'people/olga-petrova').text).toBe(
			'# Olga\n\n- Lives in Tver\n- Also called: Grandma\n'
		);

		moveProfileNote(profile, 'people/olga-petrova', 'other');
		expect(note()).toBe('people/olga.md');
	});

	it("are never merged into each other's", () => {
		const { profile } = makeFamily();
		makeUser('Max');
		addMemberWithNote(profile, 'Max');
		membersWithNotes(profile);
		addMemoryFact(profile.slug, 'people/anna', 'Likes tea');
		addMemoryFact(profile.slug, 'people/max', 'Plays chess');
		expect(() => mergeProfileNotes(profile, 'people/max', 'people/anna')).toThrow(
			"people/max.md is Max's note and people/anna.md is Anna's: they're about two people."
		);
	});

	it('are what memory knows a member by, so "grandma" finds what hers says', async () => {
		const { profile } = makeFamily();
		const olgaUser = makeUser('Olga');
		writeMemoryNote(profile.slug, 'people/olga', `${olga}\n## Contacts\n\n- Phone: 555-0101\n`);
		addMemberWithNote(profile, 'Olga', 'people/olga');
		expect(memberWords(profile, olgaUser.id, 'Olga')).toBe('Olga olga Olga grandma бабушка');

		const hits = await searchMemory(profile.slug, 'бабушка phone');
		expect(hits[0]).toMatchObject({ path: 'people/olga.md', text: 'Phone: 555-0101' });
	});
});
