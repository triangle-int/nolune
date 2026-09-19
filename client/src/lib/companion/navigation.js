/**
 * Primary companion navigation (#98).
 *
 * Nolune reads as a companion, not an admin dashboard: five destinations,
 * each answering one question. Drops are an outcome that shows up in chat
 * and under Activity, so they have no tab of their own; Skills are a
 * capability under Settings.
 *
 * @typedef {{ id: string, label: string, description: string }} PrimaryTab
 */

/** @type {readonly PrimaryTab[]} */
export const PRIMARY_TABS = Object.freeze([
	{ id: "chat", label: "Chat", description: "Talk with your companion." },
	{ id: "activity", label: "Activity", description: "What it did on its own, and what it made." },
	{ id: "memory", label: "Memory", description: "What it remembers about you." },
	{ id: "computers", label: "Computers", description: "Desktops connected to it." },
	{ id: "settings", label: "Settings", description: "How it behaves and what it may use." },
]);

/** Routes that belong to a tab without being one. */
const FOLDED_ROUTES = Object.freeze({ drops: "activity" });

/**
 * @param {string} slug
 * @param {string} tab
 */
export function tabHref(slug, tab) {
	return `/${slug}/${tab}`;
}

/**
 * Which primary tab a pathname belongs to. Nested routes keep their tab
 * active (`/settings/advanced` is still Settings); unknown paths are chat.
 *
 * @param {string} pathname
 * @param {string} slug
 */
export function activeTab(pathname, slug) {
	const prefix = `/${slug}/`;
	if (!pathname.startsWith(prefix)) return "chat";
	const segment = pathname.slice(prefix.length).split("/")[0];
	if (segment in FOLDED_ROUTES) return FOLDED_ROUTES[/** @type {keyof typeof FOLDED_ROUTES} */ (segment)];
	return PRIMARY_TABS.some((t) => t.id === segment) ? segment : "chat";
}
