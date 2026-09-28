import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	createConversation,
	getConversation,
	listConversations,
	replaceTitle,
	touchConversation
} from './conversations.ts';
import { renameConversation, subscribe, type LiveEvent } from './runner.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';
import { TITLE_LIMIT, TitleError, typedTitle } from './titles.ts';

afterEach(() => {
	vi.useRealTimers();
});

describe('renaming a chat', () => {
	it('keeps a typed name on one line, and refuses an empty or overlong one', () => {
		expect(typedTitle('  Trip to\n  Japan ')).toBe('Trip to Japan');
		expect(() => typedTitle(' \n ')).toThrow(TitleError);
		expect(typedTitle('a'.repeat(TITLE_LIMIT))).toHaveLength(TITLE_LIMIT);
		expect(() => typedTitle('a'.repeat(TITLE_LIMIT + 1))).toThrow(
			`at most ${TITLE_LIMIT} characters`
		);
	});

	it('tells everyone who has the chat open, and leaves it where it is in the list', () => {
		vi.useFakeTimers({ toFake: ['Date'] });
		vi.setSystemTime(Date.UTC(2026, 8, 1));
		const { user, profile } = makeFamily();
		const preset = makePreset();
		const make = (title: string) =>
			createConversation({ profile, presetId: preset.id, userId: user.id, title });
		const older = make('Older');
		vi.advanceTimersByTime(60_000);
		make('Newer');

		const events: LiveEvent[] = [];
		const unsubscribe = subscribe(older.id, (event) => events.push(event));
		renameConversation(older.id, ' Trip  plans ');
		unsubscribe();

		expect(events).toEqual([{ type: 'title', title: 'Trip plans' }]);
		expect(listConversations(profile.id).map((c) => c.title)).toEqual(['Newer', 'Trip plans']);
		expect(() => renameConversation(older.id, '')).toThrow(TitleError);
		expect(getConversation(older.id)?.title).toBe('Trip plans');
	});

	it("keeps the new name when nolune's own title for the chat arrives later", () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		// The first message stands in as the title while the model thinks of one.
		touchConversation(chat.id, 'what should we cook tonight');
		renameConversation(chat.id, 'Dinner');
		expect(replaceTitle(chat.id, 'what should we cook tonight', 'Dinner ideas')).toBe(false);
		expect(getConversation(chat.id)?.title).toBe('Dinner');
	});
});
