import { MAX_CARD_CHARS, cardFiles, cardOf, findUser, listUsers, readCard } from '@nolune/core';
import type { Io } from './io.ts';

export const CARD_HELP = `Cards (a note per user that goes with them into all their profiles; at most ${MAX_CARD_CHARS} characters)
  nolune card [<name|email>]                    print someone's card, or list everyone's
  People edit their own card on its page in the web app; in a chat, nolune changes one with
  nolune memory, only from what its owner says about themselves.`;

export function cardCommand(io: Io, args: string[]): void {
	const [who] = args;
	if (who === 'help') {
		io.log(CARD_HELP);
		return;
	}
	if (!who) {
		const users = listUsers();
		if (!users.length) {
			io.log('No users yet.');
			return;
		}
		const cards = users.map((u) => cardOf(u.id));
		const files = new Map(cardFiles(cards).map((file) => [file.path, file]));
		const width = Math.max(...cards.map((card) => card.path.length));
		for (const card of cards) {
			const facts = files.get(card.path)?.facts.length ?? 0;
			io.log(
				`  ${card.path.padEnd(width)}  ${facts ? `${facts} fact${facts === 1 ? '' : 's'}` : 'empty'}  (${card.owner})`
			);
		}
		return;
	}
	const found = findUser(who.trim());
	if (!found) throw new Error(`No user "${who}"`);
	const card = cardOf(found.id);
	const note = readCard(card);
	if (!note?.text.trim()) {
		io.log(`${card.owner}'s card (${card.path}) is empty so far.`);
		return;
	}
	io.stdout(note.text.endsWith('\n') ? note.text : `${note.text}\n`);
}
