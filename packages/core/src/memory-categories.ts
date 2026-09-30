/*
 * The categories memory keeps things in, so every fact has one place to go and the Memory page
 * reads the same in every family: notes of their own (core, home, health...), and folders with a
 * note each (one per person, one per project). Facts are only added to these; a note from before
 * them stays readable and editable until it's moved into one (`nolune memory mv`). Pure logic:
 * memory.ts refuses the rest.
 */

export type MemoryCategory =
	| 'core'
	| 'people'
	| 'home'
	| 'health'
	| 'plans'
	| 'routines'
	| 'pets'
	| 'places'
	| 'projects'
	| 'other';

/** In the order the Memory page shows them. */
export const MEMORY_CATEGORIES: MemoryCategory[] = [
	'core',
	'people',
	'home',
	'health',
	'plans',
	'routines',
	'pets',
	'places',
	'projects',
	'other'
];

/** Categories that are a folder of notes, one per person or project. `projects` is also a note. */
const FOLDERS: MemoryCategory[] = ['people', 'projects'];
const NOTES: MemoryCategory[] = MEMORY_CATEGORIES.filter((c) => c !== 'people');

/** What goes in each, for the agent and the note-taker. */
export const CATEGORY_HINTS: Record<MemoryCategory, string> = {
	core: "pinned, in every conversation whole, so only what matters in nearly every one: who is in the family and how to address them, the languages they use, allergies, standing preferences (a member's own go on their card)",
	people:
		"one note per person, in the family or not (grandparents, friends, the nanny, doctors, teachers): who they are to the family, what they're called, how to reach them, birthdays, what they like",
	home: 'the home and household: devices, accounts and services, where things are kept, codes',
	health: "conditions, medicines, doctors (allergies also go in core, or on a member's card)",
	plans: "what's coming up, with full dates: events, trips, appointments, deadlines",
	routines: 'what repeats: schedules, classes, chores, habits, meals',
	pets: 'the pets',
	places: 'addresses and places: schools, shops, favorite spots',
	projects: 'one note per ongoing project, or projects for small ones',
	other: 'only what fits nowhere else'
};

/** The categories as a list, for the agent's and the note-taker's prompts. */
export function categoryGuide(): string {
	return MEMORY_CATEGORIES.map(
		(c) => `- ${FOLDERS.includes(c) ? `${c}/<name>` : c}: ${CATEGORY_HINTS[c]}`
	).join('\n');
}

/** How a person's note starts, for the prompts. */
export const PERSON_NOTE_GUIDE =
	"A person's note is titled with their name (`# Olga`). When you know them, it also says who they are to the family and what else the family calls them, each a fact of its own, so nolune knows them by any of those names: `- Who: Anna's grandmother`, `- Also called: grandma, бабушка` (keep the labels Who and Also called as they are). Leave out what you don't know: no Who that says nothing, like \"family member\", and no Also called that only repeats their name.";

/** A note's path without its extension: `people/anna.md` is `people/anna`. */
function stem(path: string): string {
	return path.replace(/\.(md|markdown|txt)$/i, '');
}

/**
 * The category a note is in, from its path in the memory folder; null for a note outside them
 * (from before, or somewhere else).
 */
export function categoryOf(path: string): MemoryCategory | null {
	const parts = stem(path).split('/');
	const top = parts[0].toLowerCase() as MemoryCategory;
	if (parts.length === 1) return NOTES.includes(top) ? top : null;
	return parts.length === 2 && parts[1] && FOLDERS.includes(top) ? top : null;
}

/**
 * Where the cards (memory-cards.ts) are in a profile's memory: `cards/anna.md` is a member's card,
 * a note of its own that goes with them into all their profiles. Not a category: nothing of the
 * profile's own goes there.
 */
export const CARDS_FOLDER = 'cards';

/** Whether a note's path (or topic) is a card's, like `cards/anna` or `cards/anna.md`. */
export function isCardPath(path: string): boolean {
	const parts = stem(path.trim().replace(/^\/+|\/+$/g, '')).split('/');
	return parts.length === 2 && parts[0] === CARDS_FOLDER && !!parts[1];
}

/** Why a note can't be written to, for the agent: the categories, and where things go. */
export function categoryProblem(path: string, exists: boolean): string {
	const topic = stem(path);
	const notes = NOTES.join(', ');
	const move = exists
		? ` To keep adding to it, move it into one first: \`nolune memory mv ${topic} <category>\`.`
		: '';
	return `"${topic}" isn't one of memory's categories: ${notes}, or a note of its own in people/ or projects/ (like people/anna). Put each fact where it fits: what's about a person in their people/ note, dates in plans.${move}`;
}

// --- People ---

/** A person's or project's name as a note name: `Anna Smith` is `anna-smith`. */
export function noteName(name: string): string {
	return (
		name
			.normalize('NFKD')
			.replace(/\p{M}/gu, '')
			.toLowerCase()
			.replace(/[^\p{L}\p{N}]+/gu, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 40) || 'someone'
	);
}

/** How a person's note says who they are to the family, in any of nolune's languages. */
const WHO = /^(?:who|кто|wer|qui[eé]n|qui)\s*:\s*(.+)$/i;

/** How a person's note names what the family calls them, in any of nolune's languages. */
const ALSO_CALLED =
	/^(?:also called|aka|a\.k\.a\.|зовут|также|auch genannt|genannt|también llamad[oa]|apodo|aussi appelée?|surnom)\s*:\s*(.+)$/i;

/** A line like `- Also called: grandma, бабушка`. */
export function isAliasLine(line: string): boolean {
	return ALSO_CALLED.test(line.replace(/^\s*[-*+]\s+/, '').trim());
}

/**
 * What a person's note says the family calls them, from its lines like `Also called: grandma,
 * Granny Olga` (or `Зовут: бабушка`), without the note's own title.
 */
export function aliasesOf(text: string): string[] {
	const names: string[] = [];
	for (const line of text.split('\n')) {
		const match = line
			.replace(/^\s*[-*+]\s+/, '')
			.trim()
			.match(ALSO_CALLED);
		if (!match) continue;
		for (const name of match[1].split(/[,;]/)) {
			const clean = name.replace(/[*_`]/g, '').trim();
			if (clean && !names.includes(clean)) names.push(clean);
		}
	}
	return names;
}

/** Who a person's note says they are to the family, from its line like `Who: Anna's grandmother`. */
export function whoIn(text: string): string | null {
	for (const line of text.split('\n')) {
		const match = line.replace(/^\s*[-*+]\s+/, '').match(WHO);
		if (match) return match[1].replace(/[*_`]/g, '').trim() || null;
	}
	return null;
}

/** A note's title, its first `# heading`, or null. */
export function titleIn(text: string): string | null {
	const match = text.match(/^\s*#\s+(.+?)\s*#*\s*$/m);
	return match ? match[1].trim() : null;
}

const CYRILLIC: Record<string, string> = {
	а: 'a',
	б: 'b',
	в: 'v',
	г: 'g',
	д: 'd',
	е: 'e',
	ё: 'e',
	ж: 'zh',
	з: 'z',
	и: 'i',
	й: 'y',
	к: 'k',
	л: 'l',
	м: 'm',
	н: 'n',
	о: 'o',
	п: 'p',
	р: 'r',
	с: 's',
	т: 't',
	у: 'u',
	ф: 'f',
	х: 'kh',
	ц: 'ts',
	ч: 'ch',
	ш: 'sh',
	щ: 'shch',
	ъ: '',
	ы: 'y',
	ь: '',
	э: 'e',
	ю: 'yu',
	я: 'ya'
};

/**
 * A name as it's compared: lower case, without accents, in Latin letters (Ольга and Olga are the
 * same), without what isn't a letter or digit.
 */
export function nameKey(name: string): string {
	return [...name.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase()]
		.map((c) => CYRILLIC[c] ?? c)
		.join('')
		.replace(/[^\p{L}\p{N}]+/gu, '');
}
