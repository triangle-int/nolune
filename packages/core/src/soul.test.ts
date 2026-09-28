import { readFileSync, writeFileSync } from 'node:fs';
import { eq } from 'drizzle-orm';
import { describe, expect, it } from 'vitest';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	getConversation,
	insertQueued
} from './conversations.ts';
import { getDb } from './db/index.ts';
import { conversation } from './db/schema.ts';
import { profileSoulFile } from './paths.ts';
import { withCurrentContext } from './runner.ts';
import { MAX_SOUL_CHARS, readSoul, readSoulFile, writeSoul } from './soul.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

function say(conversationId: string, user: { id: string; name: string }, text: string) {
	insertQueued({ conversationId, senderId: user.id, senderName: user.name, text });
	commitQueuedRows(conversationId);
}

function reply(conversationId: string, text: string) {
	return appendRow({
		conversationId,
		role: 'assistant',
		kind: 'assistant',
		content: JSON.stringify([{ type: 'text', text }])
	});
}

describe('the soul', () => {
	it('opens the prompt of every new chat', () => {
		const { user, profile } = makeFamily();
		writeSoul(profile.slug, 'You are calm and curious.');
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		const soul = chat.systemPrompt.indexOf('<soul>\nYou are calm and curious.\n</soul>');
		expect(soul).toBeGreaterThan(0);
		expect(soul).toBeLessThan(chat.systemPrompt.indexOf('# Conversations'));
		expect(chat.soul).toBe('You are calm and curious.');
	});

	it("is offered while there isn't one", () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		expect(chat.systemPrompt).toContain("This profile hasn't given you a soul yet");
		expect(chat.systemPrompt).toContain('nolune soul write');
		expect(chat.soul).toBe('');
	});

	it('is saved trimmed, stays short, and goes away when emptied', () => {
		const { profile } = makeFamily();
		expect(writeSoul(profile.slug, '  Kind.\r\nPatient.\r\n\r\n')).toBe('Kind.\nPatient.');
		expect(readFileSync(profileSoulFile(profile.slug), 'utf8')).toBe('Kind.\nPatient.\n');
		expect(() => writeSoul(profile.slug, 'x'.repeat(MAX_SOUL_CHARS + 1))).toThrow(
			`at most ${MAX_SOUL_CHARS}`
		);
		expect(readSoulFile(profile.slug)).toBe('Kind.\nPatient.');
		writeSoul(profile.slug, ' \n');
		expect(readSoulFile(profile.slug)).toBe('');
	});

	it('is cut at a line in the prompt when it grew too long in an editor', () => {
		const { user, profile } = makeFamily();
		const line = `${'y'.repeat(99)}\n`;
		const text = line.repeat(Math.ceil(MAX_SOUL_CHARS / line.length) + 5).trim();
		writeFileSync(profileSoulFile(profile.slug), text);
		const soul = readSoul(profile.slug);
		expect(soul.cut).toBe(true);
		expect(soul.text.length).toBeLessThanOrEqual(MAX_SOUL_CHARS);
		expect(readSoulFile(profile.slug)).toBe(text);
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		expect(chat.systemPrompt).toContain('the rest was cut off here');
	});

	it('reaches open chats at their next message', () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		say(chat.id, user, 'Hi');
		const first = reply(chat.id, 'Hello!');

		writeSoul(profile.slug, 'You answer in rhymes.');
		say(chat.id, user, 'How are you?');
		const changed = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(changed.systemPrompt).toContain('<soul>\nYou answer in rhymes.\n</soul>');
		expect(changed.soul).toBe('You answer in rhymes.');
		expect(changed.promptChangedAtSeq).toBe(first.seq);
		expect(getConversation(chat.id)).toEqual(changed);
		// Up to date now: nothing changes on the next call.
		expect(withCurrentContext(changed, committedRows(chat.id))).toBe(changed);

		const second = reply(chat.id, 'Fine, and you, as the sun shines through?');
		writeSoul(profile.slug, '');
		say(chat.id, user, 'Stop rhyming');
		const removed = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(removed.systemPrompt).not.toContain('<soul>');
		expect(removed.promptChangedAtSeq).toBe(second.seq);
	});

	it('leaves chats from before souls alone until there is one', () => {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		// As the migration leaves a chat that was made before: a prompt without a soul section.
		getDb()
			.update(conversation)
			.set({ systemPrompt: 'An old prompt.' })
			.where(eq(conversation.id, chat.id))
			.run();
		say(chat.id, user, 'Hi');
		const old = getConversation(chat.id)!;
		expect(withCurrentContext(old, committedRows(chat.id))).toBe(old);
	});
});
