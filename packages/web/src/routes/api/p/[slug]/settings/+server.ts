import { error, json } from '@sveltejs/kit';
import {
	MAX_SOUL_CHARS,
	SoulError,
	listPersonNotes,
	listUsers,
	membersWithNotes,
	readSoulFile,
	renameProfile,
	setProfileAvatar,
	writeSoul
} from '@nolune/core';
import { translations } from '$lib/i18n';
import { requireProfile } from '$lib/server/access';
import type { RequestHandler } from './$types';

/**
 * The profile as its People & profile page shows it: its name and avatar, its members with whose
 * note in memory is theirs, the people notes to choose from, who else could join, and its soul.
 */
export const GET: RequestHandler = ({ params, locals }) => {
	const { profile } = requireProfile(locals, params.slug);
	const members = membersWithNotes(profile);
	const memberIds = new Set(members.map((m) => m.id));
	return json({
		name: profile.name,
		avatar: profile.avatar,
		members,
		people: listPersonNotes(profile.slug),
		others: listUsers()
			.filter((u) => !memberIds.has(u.id))
			.map((u) => u.name),
		soul: readSoulFile(profile.slug),
		maxSoul: MAX_SOUL_CHARS
	});
};

/** Renames the profile (`{ name }`), picks its avatar (`{ avatar }`) or writes its soul (`{ soul }`). */
export const PATCH: RequestHandler = async ({ params, locals, request }) => {
	const { profile } = requireProfile(locals, params.slug);
	const { m } = translations(locals.locale);
	const body = (await request.json().catch(() => null)) as {
		name?: unknown;
		avatar?: unknown;
		soul?: unknown;
	} | null;
	try {
		if (typeof body?.name === 'string') {
			renameProfile(profile.id, body.name);
			return json({ message: m.profile.renamed });
		}
		if (typeof body?.avatar === 'string') {
			setProfileAvatar(profile.id, body.avatar);
			return json({});
		}
		if (typeof body?.soul === 'string') {
			return json({
				message: writeSoul(profile.slug, body.soul) ? m.profile.soulSaved : m.profile.soulRemoved
			});
		}
	} catch (err) {
		if (err instanceof SoulError || err instanceof Error) error(400, err.message);
		throw err;
	}
	error(400, 'Send a name, an avatar or a soul');
};
