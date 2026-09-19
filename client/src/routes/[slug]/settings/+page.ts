import { redirect } from "@sveltejs/kit";
import { defaultSection, sectionHref } from "$lib/settings/sections.js";
import type { PageLoad } from "./$types";

// /settings has no content of its own (#98): it opens the first section.
export const load: PageLoad = ({ params }) => {
	redirect(307, sectionHref(params.slug, defaultSection()));
};
