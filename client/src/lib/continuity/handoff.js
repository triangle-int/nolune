// @ts-check
/**
 * Pure helpers for handoff cards (#82). The server's card is the only
 * source: it is derived from the continuity record and the known machines,
 * never from model text. Nothing here reads a store or talks to the API;
 * the components decide what to fetch and when.
 */

import { relativeTime } from "../activity/receipts.js";

/**
 * @typedef {{
 *   machine_id: string;
 *   display_name: string;
 *   known: boolean;
 *   online: boolean;
 *   health: "healthy" | "degraded" | "unavailable";
 *   platform: string | null;
 *   last_seen: number | null;
 * }} ComputerSummary
 */
/** @typedef {{ status: "completed" | "failed" | "cancelled"; finished_at: number; summary: string }} HandoffOutcome */
/**
 * @typedef {(
 *   | { kind: "accepted"; machine_id: string; run_id: string; at: number; outcome?: HandoffOutcome | null }
 *   | { kind: "kept"; machine_id?: string | null; at: number }
 *   | { kind: "dismissed"; at: number }
 * )} HandoffDecision
 */
/** @typedef {{ capabilities: string[]; permissions: string[] }} Requirements */
/**
 * @typedef {{
 *   record_id: string;
 *   goal: string;
 *   state: string;
 *   origin_chat_id: string;
 *   origin: ComputerSummary | null;
 *   completed_steps: string[];
 *   resources: { resource: { kind: string; [key: string]: unknown }; label: string; available: boolean }[];
 *   blockers: string[];
 *   next_step: string | null;
 *   required: Requirements;
 *   decision: HandoffDecision | null;
 *   bound_to: ComputerSummary | null;
 *   offered: boolean;
 *   created_at: number;
 *   updated_at: number;
 * }} HandoffCard
 */
/** @typedef {{ kind: { kind: string; [key: string]: unknown }; severity: "blocking" | "approval" | "note"; detail: string }} ContinuationCheck */
/**
 * The subset of the known-machine row the card needs (`GET /machines`).
 * @typedef {{ machine_id: string; display_name: string; online: boolean; health: "healthy" | "degraded" | "unavailable" }} MachineRow
 */

/** @param {string} permission */
function permissionLabel(permission) {
	switch (permission) {
		case "screen_capture":
			return "Screen Recording";
		case "accessibility":
			return "Accessibility";
		default:
			return permission;
	}
}

/** A computer that can take work right now. @param {{ online: boolean; health: string }} m */
function ready(m) {
	return m.online && m.health !== "degraded";
}

/**
 * The origin line: where the task started and, when that matters, what
 * state that computer is in. It reads the same whether or not the origin
 * is connected, so the card stays useful while it is offline.
 * @param {HandoffCard} card
 * @param {number} now unix seconds
 */
export function originCopy(card, now) {
	const origin = card.origin;
	if (!origin) return "Started in chat, no computer yet";
	const name = origin.display_name;
	if (!origin.known) return `Started on ${name} · not connected to this companion`;
	if (origin.online && origin.health === "degraded") return `Started on ${name} · connected but not responding`;
	if (!origin.online) {
		const seen = origin.last_seen === null ? "" : `, last seen ${relativeTime(origin.last_seen, now)}`;
		return `Started on ${name} · offline${seen}`;
	}
	return `Started on ${name}`;
}

/**
 * The name of a computer the decision mentions: the bound computer's
 * summary when the card carries one, the origin's when it is the origin,
 * else the id.
 * @param {HandoffCard} card
 * @param {string} machineId
 */
function computerName(card, machineId) {
	if (card.bound_to?.machine_id === machineId) return card.bound_to.display_name;
	if (card.origin?.machine_id === machineId) return card.origin.display_name;
	return machineId;
}

/**
 * The decision line: which computer the task is bound to and how the
 * continuation ended. Outcomes are the server's receipts, never model text.
 * @param {HandoffCard} card
 */
export function decisionLabel(card) {
	const decision = card.decision;
	if (!decision) return "";
	switch (decision.kind) {
		case "accepted": {
			const name = computerName(card, decision.machine_id);
			const outcome = decision.outcome;
			if (!outcome) return `Continuing on ${name}`;
			switch (outcome.status) {
				case "completed":
					return `Continued on ${name} · ${outcome.summary}`;
				case "failed":
					return `Stopped on ${name} · ${outcome.summary}`;
				default:
					return `Cancelled on ${name}`;
			}
		}
		case "kept":
			return decision.machine_id ? `Kept on ${computerName(card, decision.machine_id)}` : "Kept where it is";
		default:
			return "Dismissed";
	}
}

/**
 * Which actions the card offers. A continuation that is still running
 * cannot be started twice, and a card that is not offered has nothing to
 * decide.
 * @param {HandoffCard} card
 */
export function cardActions(card) {
	const continuing = card.decision?.kind === "accepted" && !card.decision.outcome;
	const open = card.offered && !continuing;
	return { continueHere: open, continueOn: open, keepThere: open, dismiss: open, continuing };
}

/**
 * The computers to pick from for "Continue on…": ready ones first (the
 * origin ahead of the rest), then the ones that cannot take work, each
 * saying why.
 * @param {MachineRow[]} machines
 * @param {string | null | undefined} originId
 * @returns {{ machine_id: string; label: string; available: boolean }[]}
 */
export function machineChoices(machines, originId) {
	const rank = (/** @type {MachineRow} */ m) => {
		if (ready(m)) return m.machine_id === originId ? 0 : 1;
		return m.online ? 2 : 3;
	};
	return machines
		.map((m, index) => ({ m, index }))
		.sort((a, b) => rank(a.m) - rank(b.m) || a.index - b.index)
		.map(({ m }) => {
			const parts = [m.display_name];
			if (m.machine_id === originId) parts.push("where it started");
			if (m.online && m.health === "degraded") parts.push("not responding");
			else if (!m.online) parts.push("offline");
			return { machine_id: m.machine_id, label: parts.join(" · "), available: ready(m) };
		});
}

/**
 * Which computer "here" is: the one the user chose before, when it is
 * connected; otherwise the only connected one; otherwise nothing, and the
 * card asks.
 * @param {MachineRow[]} machines
 * @param {string | null} remembered
 * @returns {string | null}
 */
export function resolveHere(machines, remembered) {
	if (remembered) {
		return machines.some((m) => m.machine_id === remembered && m.online) ? remembered : null;
	}
	const connected = machines.filter((m) => m.online);
	return connected.length === 1 ? connected[0].machine_id : null;
}

/**
 * The preview's checks by what they mean for the user: what stops the
 * continuation, what the desktop will ask for, and what is only worth
 * knowing.
 * @param {ContinuationCheck[]} checks
 */
export function groupChecks(checks) {
	/** @type {{ blocking: string[]; approvals: string[]; notes: string[] }} */
	const grouped = { blocking: [], approvals: [], notes: [] };
	for (const check of checks) {
		if (check.severity === "blocking") grouped.blocking.push(check.detail);
		else if (check.severity === "approval") grouped.approvals.push(check.detail);
		else grouped.notes.push(check.detail);
	}
	return grouped;
}

/**
 * What the destination must offer, as one sentence.
 * @param {Requirements} required
 */
export function requiredSummary(required) {
	const parts = [];
	if (required.capabilities.length > 0) parts.push(required.capabilities.join(", "));
	if (required.permissions.length > 0) parts.push(`${required.permissions.map(permissionLabel).join(" and ")} permissions`);
	return parts.length === 0 ? "" : `Needs ${parts.join(" · ")}`;
}

/**
 * Replace or add one card, newest update first; a card that is no longer
 * offered leaves the list.
 * @template {{ record_id: string; offered: boolean; updated_at: number }} T
 * @param {T[]} cards
 * @param {T} card
 * @returns {T[]}
 */
export function upsertCard(cards, card) {
	const others = cards.filter((c) => c.record_id !== card.record_id);
	const next = card.offered ? [...others, card] : others;
	return next.sort((a, b) => b.updated_at - a.updated_at);
}
