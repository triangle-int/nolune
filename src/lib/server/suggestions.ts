import type { IconNode } from 'lucide';
import type { Suggestion } from '@btw/core';
import { lucideIcon } from './icons';

/** A new-chat chip with its icon's drawing, so the page shows it without fetching it first. */
export interface ShownSuggestion {
	icon: IconNode;
	label: string;
	text: string;
}

export function withIcons(suggestions: Suggestion[]): ShownSuggestion[] {
	return suggestions.map((s) => ({
		...s,
		// The model picks the name; one Lucide doesn't have gets a generic icon.
		icon: lucideIcon(s.icon) ?? lucideIcon('sparkles') ?? []
	}));
}
