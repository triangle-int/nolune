import { fail, redirect } from '@sveltejs/kit';
import {
	addMember,
	deleteProfile,
	listMembers,
	listUsers,
	removeMember,
	renameProfile
} from '@btw/core';
import { requireProfile } from '$lib/server/access';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = ({ locals, params }) => {
	const { profile } = requireProfile(locals, params.slug);
	const members = listMembers(profile.id);
	const memberIds = new Set(members.map((m) => m.id));
	return {
		members,
		others: listUsers()
			.filter((u) => !memberIds.has(u.id))
			.map((u) => u.name)
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
		return { message: 'Renamed.' };
	},
	add: async ({ locals, params, request }) => {
		const { profile } = requireProfile(locals, params.slug);
		const who = (await request.formData()).get('who')?.toString() ?? '';
		try {
			addMember(profile.id, who);
		} catch (err) {
			return fail(400, { message: message(err) });
		}
		return { message: `Added ${who}.` };
	},
	remove: async ({ locals, params, request }) => {
		const { user, profile } = requireProfile(locals, params.slug);
		const userId = (await request.formData()).get('userId')?.toString() ?? '';
		removeMember(profile.id, userId);
		if (userId === user.id) redirect(303, '/');
		return { message: 'Removed.' };
	},
	delete: async ({ locals, params }) => {
		const { profile } = requireProfile(locals, params.slug);
		deleteProfile(profile.id);
		redirect(303, '/');
	}
};
