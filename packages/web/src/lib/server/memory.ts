import {
	CORE_NOTE,
	MAX_PINNED_CHARS,
	cardCandidates,
	cardFiles,
	cardProfiles,
	listMemoryFiles,
	membersWithNotes,
	profileCards,
	recentMemoryChanges
} from '@nolune/core';
import type { requireProfile } from './access';

type Person = ReturnType<typeof requireProfile>['user'];
type Profile = ReturnType<typeof requireProfile>['profile'];

const RECENT_DAYS = 14;

/**
 * A profile's memory as its page shows it, and apps read it (`/api/p/<slug>/memory`): its notes,
 * the members' cards, what the note-taker saved lately, and whose notes are whose.
 */
export function memoryOverview(user: Person, profile: Profile) {
	const cards = profileCards(profile.id);
	const cardFile = new Map(cardFiles(cards).map((file) => [file.path, file]));
	const mine = cards.find((card) => card.userId === user.id);
	const mineEmpty = !mine || !cardFile.get(mine.path)?.facts.length;
	return {
		files: listMemoryFiles(profile.slug),
		// The members' cards, which go with each into all their profiles.
		cards: cards.map((card) => ({
			path: card.path,
			owner: card.owner,
			ownerId: card.userId,
			mine: card.userId === user.id,
			file: cardFile.get(card.path) ?? null
		})),
		// Only for the viewer's own card: who reads it, and what their notes could start it with.
		myProfiles: cardProfiles(user.id).map((p) => p.name),
		toBringIn: mineEmpty ? cardCandidates(user.id).length : 0,
		// What the note-taker saved lately, where it came from, with Undo.
		recent: recentMemoryChanges(profile.id, {
			since: new Date(Date.now() - RECENT_DAYS * 24 * 60 * 60 * 1000),
			limit: 30
		}),
		core: { path: CORE_NOTE, maxChars: MAX_PINNED_CHARS },
		// Whose notes are whose: members' come first under People.
		members: membersWithNotes(profile).flatMap((m) =>
			m.note ? [{ id: m.id, name: m.name, note: m.note }] : []
		),
		learnFromChats: profile.learnFromChats
	};
}
