/**
 * Settings ownership (#98).
 *
 * Every retained setting has exactly one section and one owner:
 * `server` settings apply to this whole Nolune server; `companion` settings
 * describe the one companion it hosts. Settings marked `raw` are server or
 * protocol wiring (ports, tokens, hosts, routing). They stay reachable for
 * self-hosters under Advanced and never appear on the other pages, so common
 * setup finishes without seeing them.
 *
 * @typedef {"server" | "companion"} Scope
 * @typedef {{ id: string, label: string, description: string }} Section
 * @typedef {{ key: string, label: string, section: string, scope: Scope, raw?: boolean }} Setting
 */

/** @type {readonly Section[]} */
export const SETTINGS_SECTIONS = Object.freeze([
	{ id: "companion", label: "Companion", description: "Presence, rhythm, initiative, and time." },
	{ id: "connections", label: "Connections", description: "Model presets, API keys, your computers, and paired browsers." },
	{ id: "capabilities", label: "Capabilities", description: "Skills and reviewed extensions it may use." },
	{ id: "data", label: "Data", description: "What Nolune keeps, and how to take it with you." },
	{ id: "advanced", label: "Advanced", description: "Server, updates, and integration details for self-hosters." },
]);

/** @type {readonly Setting[]} */
export const SETTINGS = Object.freeze([
	{ key: "presence", label: "Little Moon", section: "companion", scope: "companion" },
	{ key: "rhythm", label: "Learn my rhythm", section: "companion", scope: "companion" },
	{ key: "initiative", label: "Initiative", section: "companion", scope: "companion" },
	{ key: "timezone", label: "Timezone", section: "companion", scope: "companion" },
	{ key: "scheduled", label: "Scheduled messages", section: "companion", scope: "companion" },

	{ key: "models", label: "Model presets", section: "connections", scope: "server" },
	{ key: "api-keys", label: "API keys", section: "connections", scope: "server" },
	{ key: "computers", label: "Connected computers", section: "connections", scope: "server" },
	{ key: "paired-browsers", label: "Paired browsers", section: "connections", scope: "server" },

	{ key: "skills", label: "Skills", section: "capabilities", scope: "server" },
	{ key: "extensions", label: "Extensions", section: "capabilities", scope: "server" },

	{ key: "export", label: "Export", section: "data", scope: "companion" },
	{ key: "import", label: "Import", section: "data", scope: "companion" },

	{ key: "server.port", label: "Port", section: "advanced", scope: "server", raw: true },
	{ key: "server.auth-token", label: "API token", section: "advanced", scope: "server", raw: true },
	{ key: "updates", label: "Updates", section: "advanced", scope: "server", raw: true },
	{ key: "voice-id", label: "Voice ID", section: "advanced", scope: "companion", raw: true },
	{ key: "email", label: "Email (SMTP/IMAP)", section: "advanced", scope: "companion", raw: true },
	{ key: "github", label: "GitHub token", section: "advanced", scope: "server", raw: true },
]);

export function defaultSection() {
	return SETTINGS_SECTIONS[0].id;
}

/**
 * @param {string} slug
 * @param {string} section
 */
export function sectionHref(slug, section) {
	return `/${slug}/settings/${section}`;
}

/**
 * The section a settings pathname opens; anything unknown is the first one.
 * @param {string} pathname
 */
export function sectionForPath(pathname) {
	const match = /\/settings\/([^/]+)/.exec(pathname);
	const id = match?.[1];
	return id && SETTINGS_SECTIONS.some((s) => s.id === id) ? id : defaultSection();
}

/** @param {string} section */
export function settingsIn(section) {
	return SETTINGS.filter((s) => s.section === section);
}

/** @param {Scope} scope */
export function ownerLabel(scope) {
	return scope === "server" ? "This server" : "Your companion";
}
