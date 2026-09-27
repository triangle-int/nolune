import type { IconNode } from 'lucide';
import type { Suggestion } from '@btw/core';
import type { Messages } from '$lib/i18n';
import { lucideIcon } from './icons';

/** A new-chat chip with its icon's drawing, so the page shows it without fetching it first. */
export interface ShownSuggestion {
	icon: IconNode;
	label: string;
	text: string;
}

export function withIcons(suggestions: Suggestion[], m: Messages): ShownSuggestion[] {
	return suggestions.map(({ id, icon, label, text }) => ({
		// The general chips in the reader's language; the model's are in the family's already.
		...(id ? m.newChat.suggestions[id] : { label, text }),
		// The model picks the name; one Lucide doesn't have gets a generic icon.
		icon: lucideIcon(icon) ?? lucideIcon('sparkles') ?? []
	}));
}
