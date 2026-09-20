// @ts-check
/**
 * Pure helpers for the Resume my work ritual (#83). The server's offer is
 * the only source: which record, why it was picked, and why now are its
 * words, never model text, and the card it leads to is the handoff card.
 * Nothing here reads a store or talks to the API; the components decide
 * what to fetch and when.
 */

import { relativeTime } from "../activity/receipts.js";
import { describeDuration } from "../commitments/commitments.js";

/** @typedef {import('./handoff.js').HandoffCard} HandoffCard */
/**
 * @typedef {(
 *   | { kind: "manual" }
 *   | { kind: "opened_after_break"; away_secs: number }
 *   | { kind: "machine_connected"; machine_id: string }
 * )} RitualTrigger
 */
/**
 * @typedef {{
 *   id: string;
 *   record_id: string;
 *   goal: string;
 *   trigger: RitualTrigger;
 *   why_now: string;
 *   why_this: string;
 *   destination_id: string;
 *   suggested_at: number;
 *   card: HandoffCard;
 * }} ResumeOffer
 */
/**
 * @typedef {{
 *   enabled: boolean;
 *   break_minutes: number;
 *   cooldown_secs: number;
 *   snooze_until: number | null;
 *   dismissed_record_ids: string[];
 * }} ResumeRitualPolicy
 */
/**
 * @typedef {(
 *   | { kind: "disabled" }
 *   | { kind: "quiet_hours" }
 *   | { kind: "cooldown"; until: number }
 *   | { kind: "snoozed"; until: number }
 *   | { kind: "no_break" }
 *   | { kind: "nothing_to_resume" }
 *   | { kind: "storage"; message: string }
 * )} Held
 */

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** How long away counts as a break, in minutes. */
export const BREAK_OPTIONS = [
	{ value: 30, label: "30 minutes" },
	{ value: 60, label: "1 hour" },
	{ value: 120, label: "2 hours" },
	{ value: 240, label: "4 hours" },
	{ value: 480, label: "8 hours" },
	{ value: 1440, label: "A day" },
];

/** The least time between two spontaneous suggestions, in seconds. */
export const COOLDOWN_OPTIONS = [
	{ value: 0, label: "None" },
	{ value: 15 * MINUTE, label: "15 minutes" },
	{ value: 30 * MINUTE, label: "30 minutes" },
	{ value: HOUR, label: "1 hour" },
	{ value: 4 * HOUR, label: "4 hours" },
	{ value: 12 * HOUR, label: "12 hours" },
	{ value: DAY, label: "A day" },
];

/**
 * The label for a stored value: the option's when it is listed, otherwise
 * the value in words so an edited file still reads.
 * @param {{ value: number; label: string }[]} options
 * @param {number} value
 */
export function optionLabel(options, value) {
	const listed = options.find((option) => option.value === value);
	if (listed) return listed.label;
	const seconds = options === BREAK_OPTIONS ? value * MINUTE : value;
	// Whole hours read as hours; anything finer is exact in minutes.
	if (seconds % HOUR !== 0 && seconds % MINUTE === 0 && seconds < DAY) return `${seconds / MINUTE} minutes`;
	return describeDuration(seconds);
}

/**
 * Fold a server event into the current offer. `undefined` means the event
 * says nothing about it; `null` clears it. A decided or withdrawn card
 * resolves the offer, since the card is where the user answers.
 * @param {ResumeOffer | null} current
 * @param {{ type: string; [key: string]: unknown }} event
 * @returns {ResumeOffer | null | undefined}
 */
export function applyResumeEvent(current, event) {
	if (event.type === "resume_updated") {
		return /** @type {ResumeOffer | null} */ (event.suggestion ?? null);
	}
	if (event.type === "handoff_updated" && current) {
		const card = /** @type {HandoffCard} */ (event.card);
		if (card.record_id !== current.record_id) return undefined;
		if (!card.offered || card.decision?.kind === "accepted") return null;
		return { ...current, card };
	}
	return undefined;
}

/**
 * When the suggestion was made, for the banner.
 * @param {ResumeOffer} offer
 * @param {number} now unix seconds
 */
export function suggestedLabel(offer, now) {
	return `Suggested ${relativeTime(offer.suggested_at, now)}`;
}

/**
 * Why there is no suggestion, as one sentence the user can act on.
 * @param {Held | undefined} held
 * @param {number} now unix seconds
 */
export function heldMessage(held, now) {
	switch (held?.kind) {
		case "disabled":
			return "Resume my work is off. Turn it on under Settings.";
		case "quiet_hours":
			return "Quiet hours are on, so nothing is suggested now.";
		case "cooldown":
			return `A suggestion was made or refused recently; the next one can come in ${describeDuration(held.until - now)}.`;
		case "snoozed":
			return `Resume my work is snoozed for ${describeDuration(held.until - now)}.`;
		case "no_break":
			return "No break long enough has passed.";
		case "storage":
			return held.message;
		default:
			return "Nothing to resume right now.";
	}
}

/** Fixed offsets from now, so the presets read the same in every timezone. @param {number} now */
export function snoozePresets(now) {
	return [
		{ label: "1 hour", until: now + HOUR },
		{ label: "4 hours", until: now + 4 * HOUR },
		{ label: "Tomorrow", until: now + DAY },
	];
}

/**
 * The snooze, while one is running.
 * @param {Pick<ResumeRitualPolicy, "snooze_until">} policy
 * @param {number} now
 */
export function snoozeStatus(policy, now) {
	const until = policy.snooze_until;
	if (typeof until !== "number" || until <= now) return "";
	return `Snoozed, ${describeDuration(until - now)} left`;
}

/**
 * One sentence saying when the ritual speaks up, for the settings hint.
 * @param {ResumeRitualPolicy} policy
 * @param {boolean} quietHours whether the companion has quiet hours set
 */
export function ritualSummary(policy, quietHours) {
	if (!policy.enabled) return "Off. Nothing is suggested until you turn it on.";
	const rate = policy.cooldown_secs > 0 ? `per ${optionLabel(COOLDOWN_OPTIONS, policy.cooldown_secs).replace(/^1 /, "").replace(/^A /, "")}` : "per trigger";
	const sentence = `After ${optionLabel(BREAK_OPTIONS, policy.break_minutes)} away, or when a computer a task names reconnects, at most one suggestion ${rate}.`;
	return quietHours ? `${sentence} Held during quiet hours.` : sentence;
}
