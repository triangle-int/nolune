import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = ({ locals }) => {
	return {
		user: locals.user
			? { id: locals.user.id, name: locals.user.name, isAdmin: locals.user.isAdmin === true }
			: null
	};
};
