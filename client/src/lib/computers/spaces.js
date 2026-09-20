// @ts-check
/**
 * Connected Spaces (#80): the pure helpers behind the Computers tab and the
 * Settings › Connections copy. A space is one place the one companion can
 * act: the server it runs on (its home) or a desktop that connected through
 * the Nolune desktop app. Every label, state and hint here is derived from a
 * `MachineInfo` record as `GET /machines` and the `machine_updated` event
 * carry it (#179); nothing is inferred beyond that record and a clock.
 *
 * Health is re-derived from `last_seen` on the caller's clock because the
 * server broadcasts a stale heartbeat only through its own watch: the worst
 * of what the server said and what the clock says wins, so a computer never
 * looks healthier here than on the server. The caller decides which clock
 * to pass; while it cannot refresh the listing it should freeze the clock at
 * the last successful refresh instead of degrading every computer it lost
 * contact with.
 */

/**
 * @typedef {import("../api/types.js").MachineInfo} MachineInfo
 * @typedef {import("../api/types.js").MachineHealth} MachineHealth
 * @typedef {import("../api/types.js").MachinePermission} MachinePermission
 * @typedef {import("../api/types.js").ServerEvent} ServerEvent
 */

/** Mirrors `STALE_HEARTBEAT_SECS` in server/src/domain/machine.rs. */
export const STALE_HEARTBEAT_SECS = 45;
/** Id of the row synthesized for the server home when the listing has none. */
export const HOME_SPACE_ID = "server-home";

/**
 * @typedef {"online" | "unhealthy" | "restricted" | "offline"} SpaceStatus
 * @typedef {"home" | "desktop"} SpaceKind
 * @typedef {{ level: "warn" | "info"; text: string }} SpaceHint
 * @typedef {{
 *   key: "accessibility" | "screen_capture";
 *   label: string;
 *   state: MachinePermission;
 *   stateLabel: string;
 *   blocking: boolean;
 * }} PermissionRow
 * @typedef {{
 *   id: string;
 *   kind: SpaceKind;
 *   name: string;
 *   customName: string | null;
 *   hostname: string;
 *   online: boolean;
 *   health: MachineHealth;
 *   status: SpaceStatus;
 *   stateLabel: string;
 *   platform: string;
 *   location: string;
 *   meta: string;
 *   lastSeen: string;
 *   permissions: PermissionRow[];
 *   capabilities: string;
 *   cua: string;
 *   hints: SpaceHint[];
 *   note: string;
 *   canRename: boolean;
 * }} SpaceView
 */

/** @type {Record<PermissionRow["key"], string>} */
const PERMISSION_LABELS = { accessibility: "Accessibility", screen_capture: "Screen recording" };

/** @type {Record<MachinePermission, string>} */
const PERMISSION_STATE_LABELS = {
	granted: "granted",
	denied: "denied",
	prompt_required: "not asked yet",
	unavailable: "not available here",
};

/** @type {Record<MachineHealth, number>} */
const HEALTH_RANK = { healthy: 0, degraded: 1, unavailable: 2 };

const CUA_NOT_REPORTED = "Cua driver not reported";

/** @param {MachineInfo} machine */
function isHome(machine) {
	return machine.location === "server_local";
}

/**
 * The health the clock derives, never better than the server's word.
 *
 * @param {MachineInfo} machine
 * @param {number} nowSeconds
 * @returns {MachineHealth}
 */
export function deriveHealth(machine, nowSeconds) {
	if (!machine.online) return "unavailable";
	/** @type {MachineHealth} */
	const fromClock = nowSeconds - machine.last_seen > STALE_HEARTBEAT_SECS ? "degraded" : "healthy";
	const fromServer = machine.health === "unavailable" ? "degraded" : machine.health;
	return HEALTH_RANK[fromServer] > HEALTH_RANK[fromClock] ? fromServer : fromClock;
}

/**
 * Permissions the desktop reported, in the order the surface shows them.
 * A machine that reported nothing has no rows: an older desktop app is not
 * four unknowns.
 *
 * @param {MachineInfo} machine
 * @returns {PermissionRow[]}
 */
export function permissionRows(machine) {
	if (!machine.permissions) return [];
	/** @type {PermissionRow["key"][]} */
	const keys = ["accessibility", "screen_capture"];
	return keys.map((key) => {
		const state = machine.permissions?.[key] ?? "unavailable";
		return {
			key,
			label: PERMISSION_LABELS[key],
			state,
			stateLabel: PERMISSION_STATE_LABELS[state],
			// `unavailable` is the platform saying it cannot report, not a refusal.
			blocking: state === "denied" || state === "prompt_required",
		};
	});
}

/**
 * Offline beats not responding beats needing permission beats online.
 *
 * @param {MachineInfo} machine
 * @param {number} nowSeconds
 * @returns {SpaceStatus}
 */
export function spaceStatus(machine, nowSeconds) {
	const health = deriveHealth(machine, nowSeconds);
	if (health === "unavailable") return "offline";
	if (health === "degraded") return "unhealthy";
	if (permissionRows(machine).some((p) => p.blocking)) return "restricted";
	return "online";
}

/**
 * @param {MachineInfo} machine
 * @param {number} nowSeconds
 */
export function stateLabel(machine, nowSeconds) {
	switch (spaceStatus(machine, nowSeconds)) {
		case "online":
			return "Online";
		case "unhealthy":
			return "Not responding";
		case "restricted":
			return "Needs permission";
		default:
			// The home row is this browser's connection to the server, so its
			// absence is a closed socket, not a server that went away.
			return isHome(machine) ? "Reconnecting" : "Offline";
	}
}

/** @param {MachineInfo} machine */
export function platformLabel(machine) {
	switch (machine.platform) {
		case "macos":
			return "macOS";
		case "windows":
			return "Windows";
		case "linux":
			return "Linux";
	}
	const os = machine.os.trim();
	const key = os.toLowerCase();
	if (key.includes("mac") || key.includes("darwin")) return "macOS";
	if (key.includes("win")) return "Windows";
	if (key.includes("linux")) return "Linux";
	return os || "Unknown platform";
}

/** @param {MachineInfo} machine */
export function locationLabel(machine) {
	return isHome(machine) ? "This server" : "Desktop";
}

/**
 * @param {MachineInfo} machine
 * @param {number} nowSeconds
 */
export function lastSeenLabel(machine, nowSeconds) {
	const age = Math.max(0, nowSeconds - machine.last_seen);
	if (machine.online) {
		return deriveHealth(machine, nowSeconds) === "degraded" ? `Last heartbeat ${age} s ago` : "Online now";
	}
	if (age < 90) return "Seen just now";
	if (age < 3600) return `Seen ${Math.round(age / 60)} min ago`;
	if (age < 86400 * 2) return `Seen ${Math.round(age / 3600)} h ago`;
	return `Seen ${Math.round(age / 86400)} days ago`;
}

/** @param {MachineInfo} machine */
export function capabilitySummary(machine) {
	const count = machine.capabilities.length;
	if (count === 0) return "No actions reported";
	return count === 1 ? "1 action" : `${count} actions`;
}

/** @param {MachineInfo} machine */
export function cuaLabel(machine) {
	if (!machine.driver_version) return CUA_NOT_REPORTED;
	const version = `Cua driver ${machine.driver_version}`;
	return machine.cua_health ? `${version} · ${machine.cua_health}` : version;
}

/**
 * Actionable hints, worst first. Each names the computer and the one thing
 * to do; an informational hint explains a field without asking for anything.
 *
 * @param {MachineInfo} machine
 * @param {number} nowSeconds
 * @returns {SpaceHint[]}
 */
export function spaceHints(machine, nowSeconds) {
	/** @type {SpaceHint[]} */
	const hints = [];
	const name = machine.display_name || machine.hostname || machine.machine_id;
	const health = deriveHealth(machine, nowSeconds);

	if (isHome(machine)) {
		if (!machine.online) {
			hints.push({
				level: "warn",
				text: "This browser lost its connection to the server. The list updates as soon as it is back.",
			});
		}
		return hints;
	}

	if (health === "unavailable") {
		hints.push({ level: "warn", text: `${name} is offline. Open the Nolune desktop app there to reconnect it.` });
	} else if (health === "degraded") {
		const age = Math.max(0, nowSeconds - machine.last_seen);
		hints.push({
			level: "warn",
			text: `${name} has not answered for ${age} s. Check that the computer is awake and the desktop app is still running.`,
		});
	}

	for (const permission of permissionRows(machine)) {
		if (permission.state === "denied") {
			hints.push({
				level: "warn",
				text: `${permission.label} is denied on ${name}. Grant it to the Nolune desktop app in System Settings, then reconnect.`,
			});
		} else if (permission.state === "prompt_required") {
			hints.push({
				level: "warn",
				text: `${permission.label} has not been allowed on ${name} yet. Open the Nolune desktop app there to allow it.`,
			});
		}
	}

	if (machine.capabilities.length === 0) {
		hints.push({ level: "warn", text: `${name} reported no actions it can perform. Update the Nolune desktop app there.` });
	}

	if (!machine.driver_version) {
		hints.push({
			level: "info",
			text: "Screen actions use the desktop app’s built-in path until the Cua driver ships.",
		});
	} else if (machine.cua_health === "degraded") {
		hints.push({ level: "warn", text: `The Cua driver on ${name} is degraded. Restart the Nolune desktop app there.` });
	} else if (machine.cua_health === "unavailable") {
		hints.push({ level: "warn", text: `The Cua driver on ${name} is unavailable. Restart the Nolune desktop app there.` });
	}

	return hints;
}

/**
 * The server home as a `MachineInfo`-shaped row, so it flows through the
 * same helpers as a desktop. Used only when the listing has no
 * `server_local` record of its own (the Cua slice may add one later).
 *
 * @param {{ connected: boolean; address?: string; version?: string; companionName?: string; nowSeconds: number }} home
 * @returns {MachineInfo}
 */
export function homeSpace({ connected, address = "", version = "", companionName = "", nowSeconds }) {
	const name = companionName.trim() || "Nolune";
	return {
		machine_id: HOME_SPACE_ID,
		display_name: `${name}’s home`,
		custom_name: null,
		hostname: address,
		// The version stands in for the OS label; the home row never shows a platform.
		os: version ? `v${version}` : "",
		platform: null,
		location: "server_local",
		screen_width: 0,
		screen_height: 0,
		permissions: null,
		capabilities: [],
		first_seen: nowSeconds,
		last_seen: nowSeconds,
		instance_slug: null,
		online: connected,
		health: connected ? "healthy" : "unavailable",
		driver_version: null,
		cua_health: null,
	};
}

/**
 * Home first, then online computers by name, then offline ones by how
 * recently they were seen.
 *
 * @param {MachineInfo[]} machines
 * @param {number} nowSeconds
 */
export function sortSpaces(machines, nowSeconds) {
	/** @param {MachineInfo} machine */
	const tier = (machine) => (isHome(machine) ? 0 : deriveHealth(machine, nowSeconds) === "unavailable" ? 2 : 1);
	return [...machines].sort((a, b) => {
		const byTier = tier(a) - tier(b);
		if (byTier !== 0) return byTier;
		if (tier(a) === 2) return b.last_seen - a.last_seen || a.machine_id.localeCompare(b.machine_id);
		return (
			a.display_name.localeCompare(b.display_name, undefined, { sensitivity: "base" }) ||
			a.machine_id.localeCompare(b.machine_id)
		);
	});
}

/**
 * @param {MachineInfo} machine
 * @param {number} nowSeconds
 * @returns {SpaceView}
 */
export function spaceView(machine, nowSeconds) {
	const home = isHome(machine);
	const companionName = home ? machine.display_name.replace(/’s home$/, "") : "";
	const meta = home
		? [locationLabel(machine), machine.hostname, machine.os].filter(Boolean).join(" · ")
		: [
				platformLabel(machine),
				machine.screen_width > 0 && machine.screen_height > 0 ? `${machine.screen_width}×${machine.screen_height}` : "",
				locationLabel(machine),
			]
				.filter(Boolean)
				.join(" · ");
	return {
		id: machine.machine_id,
		kind: home ? "home" : "desktop",
		name: machine.display_name || machine.hostname || machine.machine_id,
		customName: machine.custom_name,
		hostname: machine.hostname,
		online: machine.online,
		health: deriveHealth(machine, nowSeconds),
		status: spaceStatus(machine, nowSeconds),
		stateLabel: stateLabel(machine, nowSeconds),
		platform: home ? "" : platformLabel(machine),
		location: locationLabel(machine),
		meta,
		lastSeen: home ? "" : lastSeenLabel(machine, nowSeconds),
		permissions: home ? [] : permissionRows(machine),
		capabilities: home && machine.machine_id === HOME_SPACE_ID ? "" : capabilitySummary(machine),
		cua: home ? "" : cuaLabel(machine),
		hints: spaceHints(machine, nowSeconds),
		note: home
			? `Where ${companionName} runs. The computers below are other places it can act; they are not separate companions.`
			: "",
		canRename: !home,
	};
}

/**
 * The rows the surface renders: the listing plus the synthesized home when
 * the listing has no server-local record, sorted and viewed.
 *
 * @param {MachineInfo[]} machines
 * @param {number} nowSeconds
 * @param {MachineInfo | null | undefined} home
 * @returns {SpaceView[]}
 */
export function buildSpaces(machines, nowSeconds, home) {
	const rows = home && !machines.some(isHome) ? [home, ...machines] : machines;
	return sortSpaces(rows, nowSeconds).map((machine) => spaceView(machine, nowSeconds));
}

/**
 * Apply one server event to the listing: `machine_updated` replaces or adds
 * the row with that stable id, `machine_forgotten` drops it. Anything else
 * returns the same array, so a caller can compare identity.
 *
 * @param {MachineInfo[]} machines
 * @param {ServerEvent} event
 * @returns {MachineInfo[]}
 */
export function applyMachineEvent(machines, event) {
	if (event.type === "machine_updated") {
		const index = machines.findIndex((m) => m.machine_id === event.machine.machine_id);
		if (index === -1) return [...machines, event.machine];
		return machines.map((m, i) => (i === index ? event.machine : m));
	}
	if (event.type === "machine_forgotten") {
		return machines.some((m) => m.machine_id === event.machine_id)
			? machines.filter((m) => m.machine_id !== event.machine_id)
			: machines;
	}
	return machines;
}
