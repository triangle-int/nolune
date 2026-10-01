import { fail } from '@sveltejs/kit';
import {
	MAX_CARD_CHARS,
	MemoryConflictError,
	MemoryError,
	bringToCard,
	cardCandidates,
	cardChanges,
	cardFiles,
	cardOf,
	cardProfiles,
	forgetCardFile,
	writeCardFile
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireUser } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

/**
 * The signed-in person's card: what goes with them into all their profiles. Only they see this
 * page, with where it goes and where each change came from.
 */
export const load: PageServerLoad = ({ locals }) => {
	const user = requireUser(locals);
	const card = cardOf(user.id);
	return {
		card: { path: card.path, owner: card.owner },
		file: cardFiles([card])[0] ?? null,
		maxChars: MAX_CARD_CHARS,
		profiles: cardProfiles(user.id),
		changes: cardChanges(card),
		candidates: cardCandidates(user.id)
	};
};

function message(err: unknown): string {
	if (err instanceof MemoryError || err instanceof MemoryConflictError) return err.message;
	throw err;
}

export const actions: Actions = {
	save: async ({ locals, request }) => {
		const user = requireUser(locals);
		const { m } = translations(locals.locale);
		const form = await request.formData();
		const text = form.get('text')?.toString() ?? '';
		if (!text.trim()) return fail(400, { editing: true, message: m.card.emptyText });
		try {
			writeCardFile(cardOf(user.id), text, Number(form.get('basedOn')));
		} catch (err) {
			if (err instanceof MemoryConflictError) {
				return fail(409, {
					editing: true,
					conflict: true,
					message: m.memory.conflict(message(err))
				});
			}
			return fail(400, { editing: true, message: message(err) });
		}
		return { message: m.card.saved };
	},
	clear: async ({ locals }) => {
		const user = requireUser(locals);
		forgetCardFile(cardOf(user.id));
		return { message: translations(locals.locale).m.card.cleared };
	},
	bring: async ({ locals, request }) => {
		const user = requireUser(locals);
		const ids = (await request.formData()).getAll('fact').map(String);
		const { added, left } = bringToCard(user.id, ids);
		return { message: translations(locals.locale).m.card.bring.added(added, left) };
	}
};
