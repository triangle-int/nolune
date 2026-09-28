import { categoryOf, titleIn } from '@nolune/core/memory-categories';
import type { Messages } from '$lib/i18n';

/** A note's name as a topic: `people/anna-smith.md` is "Anna smith". */
export function memoryTopic(path: string): string {
	const name = path.replace(/\/+$/, '').split('/').pop() ?? '';
	const words = name
		.replace(/\.[^.]+$/, '')
		.replace(/[-_]+/g, ' ')
		.trim();
	return words ? words.charAt(0).toUpperCase() + words.slice(1) : 'Memory';
}

/**
 * A note's name as people read it: a category's in their language ("Планы" for plans.md), a
 * person's or project's by its title when there's `text` ("Olga" for people/grandma.md with
 * `# Olga`), anything else by its file name.
 */
export function memoryTitle(m: Messages, path: string, text?: string): string {
	const category = categoryOf(path);
	if (category && !path.includes('/')) return m.memory.categories[category];
	return (category && text && titleIn(text)) || memoryTopic(path);
}

/** Where a note's card is on the Memory page. */
export function memoryAnchor(path: string): string {
	return `memory-${path.replace(/[^a-zA-Z0-9]+/g, '-')}`;
}
