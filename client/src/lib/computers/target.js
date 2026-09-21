// @ts-check
/**
 * The composer's computer selector (#80): the pure helpers behind choosing
 * which computer the desktop tools act on for a conversation. The rows come
 * from `buildSpaces` (spaces.js), so every name and state word here is the
 * one the Computers tab shows. The choice is a stable machine id, the
 * synthesized home's id (`HOME_SPACE_ID`, which the server reads as the
 * server home) or `NO_TARGET`, which leaves the choice open: the server then
 * uses the only connected desktop and refuses to pick between several.
 *
 * The rows are `null` until the first listing for the companion arrived (or
 * while every attempt failed): not listed is not forgotten, so a remembered
 * computer is sent as it is and the server refuses it by name when it is not
 * connected (`machine_unavailable`) instead of acting on the only connected
 * one. Once listed, a computer the listing no longer has reads like no choice.
 */

import { HOME_SPACE_ID } from "./spaces.js";

/** @typedef {import("./spaces.js").SpaceView} SpaceView */
/** @typedef {import("./spaces.js").SpaceStatus} SpaceStatus */

/** Nothing chosen: the only connected desktop, or a refusal between several. */
export const NO_TARGET = "";

/**
 * @typedef {{
 *   value: string;
 *   label: string;
 *   detail: string;
 *   kind: "home" | "desktop";
 *   status: SpaceStatus;
 * }} TargetOption
 * @typedef {{
 *   name: string;
 *   detail: string;
 *   status: SpaceStatus | "ambiguous" | "pending";
 *   ambiguous: boolean;
 * }} TargetSummary
 * @typedef {SpaceView[] | null} Listing  the rows, or `null` before the first listing arrived
 */

/** What the trigger says while the listing is on its way. */
const PENDING_DETAIL = "Checking which computers are connected";

/** @param {SpaceView} space */
function detailOf(space) {
	return space.kind === "home" ? space.location : space.stateLabel;
}

/**
 * The rows as selectable options, in the order the Computers tab lists them:
 * the home first, then desktops with their state word; none before the
 * first listing arrived.
 *
 * @param {Listing} spaces
 * @returns {TargetOption[]}
 */
export function targetOptions(spaces) {
	return (spaces ?? []).map((space) => ({
		value: space.id,
		label: space.name,
		detail: detailOf(space),
		kind: space.kind,
		status: space.status,
	}));
}

/**
 * A remembered choice that the listing no longer has (a forgotten computer)
 * reads like no choice; anything listed is kept as it is, and so is
 * anything remembered while no listing has arrived yet: not listed is not
 * forgotten, and the server answers for a computer it cannot reach.
 *
 * @param {string | null | undefined} value
 * @param {Listing} spaces
 */
export function normalizeTarget(value, spaces) {
	if (!value) return NO_TARGET;
	if (spaces === null) return value;
	return spaces.some((space) => space.id === value) ? value : NO_TARGET;
}

/** @param {SpaceView[]} spaces */
function connectedDesktops(spaces) {
	return spaces.filter((space) => space.kind === "desktop" && space.online);
}

/**
 * What the desktop tools will act on, in the words the trigger shows: the
 * chosen row, else the only connected desktop, else the fact that there is
 * a choice to make or nothing to choose from. Before the first listing the
 * trigger says the choice is kept while the listing is on its way.
 *
 * @param {string | null | undefined} value
 * @param {Listing} spaces
 * @returns {TargetSummary}
 */
export function targetSummary(value, spaces) {
	const chosen = normalizeTarget(value, spaces);
	if (spaces === null) {
		return {
			name: chosen === NO_TARGET ? "Ask me" : "Remembered computer",
			detail: PENDING_DETAIL,
			status: "pending",
			ambiguous: false,
		};
	}
	if (chosen !== NO_TARGET) {
		const space = /** @type {SpaceView} */ (spaces.find((row) => row.id === chosen));
		return { name: space.name, detail: detailOf(space), status: space.status, ambiguous: false };
	}
	const connected = connectedDesktops(spaces);
	if (connected.length === 1) {
		return { name: connected[0].name, detail: "Only connected desktop", status: connected[0].status, ambiguous: false };
	}
	if (connected.length > 1) {
		return { name: "Choose a computer", detail: `${connected.length} desktops connected`, status: "ambiguous", ambiguous: true };
	}
	return { name: "No desktop connected", detail: "Open the desktop app to connect one", status: "offline", ambiguous: false };
}

/**
 * The `machine_id` the chat request carries: the stable id, the home's id,
 * or nothing.
 *
 * @param {string | null | undefined} value
 * @returns {string | null}
 */
export function requestTarget(value) {
	return value ? value : null;
}

/**
 * Where the running action is, for the chat bar: named only when one
 * computer is certain, so an open choice between several, or a listing
 * that has not arrived, says nothing.
 *
 * @param {string | null | undefined} value
 * @param {Listing} spaces
 */
export function runningLabel(value, spaces) {
	if (spaces === null) return "";
	const chosen = normalizeTarget(value, spaces);
	if (chosen === HOME_SPACE_ID || spaces.some((space) => space.id === chosen && space.kind === "home")) return "at home";
	const summary = targetSummary(chosen, spaces);
	if (summary.ambiguous || (chosen === NO_TARGET && connectedDesktops(spaces).length === 0)) return "";
	return `on ${summary.name}`;
}

/**
 * The per-viewer key the choice is remembered under, one per conversation.
 *
 * @param {string} slug
 * @param {string} chatId
 */
export function targetStorageKey(slug, chatId) {
	return `nolune:target:${slug}/${chatId}`;
}
