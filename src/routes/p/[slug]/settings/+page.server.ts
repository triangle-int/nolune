import { fail, redirect } from '@sveltejs/kit';
import {
	MAX_SOUL_CHARS,
	MemoryError,
	SoulError,
	addMemberWithNote,
	deleteProfile,
	linkPersonNote,
	listPersonNotes,
	listUsers,
	membersWithNotes,
	readSoulFile,
	removeMember,
	renameProfile,
	setProfileAvatar,
	writeSoul
} from '@btw/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const members = membersWithNotes(profile);
	const memberIds = new Set(members.map((m) => m.id));
	return {
		members,
		// For choosing which is someone's, with a look at what each says.
		people: listPersonNotes(profile.slug),
		others: listUsers()
			.filter((u) => !memberIds.has(u.id))
			.map((u) => u.name),
		soul: readSoulFile(profile.slug),
		maxSoul: MAX_SOUL_CHARS
	};
};

function message(err: unknown): string {
	return err instanceof Error ? err.message : String(err);
}

export const actions: Actions = {
	rename: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const name = (await request.formData()).get('name')?.toString() ?? '';
		try {
			renameProfile(profile.id, name);
		} catch (err) {
			return fail(400, { message: message(err) });
		}
		return { message: translations(locals.locale).m.profile.renamed };
	},
	avatar: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const avatar = (await request.formData()).get('avatar')?.toString() ?? '';
		try {
			setProfileAvatar(profile.id, avatar);
		} catch (err) {
			return fail(400, { message: message(err) });
		}
		// The picker shows the change; no message needed.
		return {};
	},
	soul: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const text = (await request.formData()).get('soul')?.toString() ?? '';
		try {
			const { m } = translations(locals.locale);
			return {
				message: writeSoul(profile.slug, text) ? m.profile.soulSaved : m.profile.soulRemoved
			};
		} catch (err) {
			if (err instanceof SoulError) return fail(400, { message: err.message });
			throw err;
		}
	},
	add: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const who = form.get('who')?.toString() ?? '';
		// Their note, once someone said which: a path, or 'new'.
		const note = form.get('note')?.toString() || undefined;
		try {
			const result = addMemberWithNote(profile, who, note);
			// Memory may know them already: ask which note is theirs before adding them.
			if (!result.added) return { choose: { who: result.name, candidates: result.candidates } };
			return { message: translations(locals.locale).m.profile.added(result.name) };
		} catch (err) {
			return fail(400, { message: message(err) });
		}
	},
	link: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const form = await request.formData();
		const userId = form.get('userId')?.toString() ?? '';
		const note = form.get('note')?.toString() ?? '';
		const member = membersWithNotes(profile).find((m) => m.id === userId);
		if (!member || !note) return fail(400, { message: 'Choose a note.' });
		try {
			const path = linkPersonNote(profile, userId, note);
			return { message: translations(locals.locale).m.profile.linked(member.name, path) };
		} catch (err) {
			if (err instanceof MemoryError) return fail(400, { message: err.message });
			throw err;
		}
	},
	remove: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const userId = (await request.formData()).get('userId')?.toString() ?? '';
		removeMember(profile.id, userId);
		if (userId === user.id) redirect(303, '/');
		return { message: translations(locals.locale).m.profile.removed };
	},
	delete: async ({ locals, params }) => {
		const { profile } = requireProfile(locals, params.slug);
		deleteProfile(profile.id);
		redirect(303, '/');
	}
};
