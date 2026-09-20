// @ts-check
/**
 * Pure helpers for commitments (#85): status and owner labels, due and wait
 * wording from a fixed clock, snooze presets, completion-evidence validation,
 * ordering, live upserts, and API refusal messages. The server's commitment
 * record is the only source; no model text ever reaches this module.
 */

/** @typedef {import("../api/types.js").Commitment} Commitment */
/** @typedef {import("../api/types.js").CommitmentCheck} CommitmentCheck */
/** @typedef {import("../api/types.js").CommitmentProvenance} CommitmentProvenance */

/** Longest evidence note the server keeps (`MAX_NOTE_CHARS`). */
export const MAX_EVIDENCE_CHARS = 200;
/** Longest promise the server keeps (`MAX_PROMISE_CHARS`). */
export const MAX_PROMISE_CHARS = 500;

const MINUTE = 60;
const HOUR = 3600;
const DAY = 86_400;
const WEEK = 7 * DAY;

/** @param {string} status */
export function statusLabel(status) {
	switch (status) {
		case "active":
			return "Active";
		case "waiting":
			return "Waiting";
		case "blocked":
			return "Blocked";
		case "due":
			return "Due";
		case "completed":
			return "Completed";
		case "dismissed":
			return "Cancelled";
		case "failed":
			return "Failed";
		default:
			return "Unknown";
	}
}

/** @param {string} status */
export function isOpen(status) {
	return status === "active" || status === "waiting" || status === "blocked" || status === "due";
}

/**
 * @param {string} owner
 * @param {string} companionName
 */
export function ownerLabel(owner, companionName = "Nolune") {
	return owner === "user" ? "You promised" : `${companionName} promised`;
}

/** @param {CommitmentProvenance} provenance */
export function provenanceLabel(provenance) {
	switch (provenance.kind) {
		case "chat":
			return "From a conversation";
		case "run":
			return "From a check-in";
		default:
			return "Added by you";
	}
}

/** Only open commitments can be edited, snoozed, completed, or cancelled. @param {Commitment} commitment */
export function canChange(commitment) {
	return isOpen(commitment.status);
}

/**
 * Whole units, floored: "under a minute", "5 minutes", "3 hours", "2 days", "2 weeks".
 * @param {number} seconds
 */
export function describeDuration(seconds) {
	const s = Math.max(0, Math.floor(seconds));
	if (s < MINUTE) return "under a minute";
	/** @param {number} n @param {string} name */
	const unit = (n, name) => `${n} ${name}${n === 1 ? "" : "s"}`;
	if (s < HOUR) return unit(Math.floor(s / MINUTE), "minute");
	if (s < DAY) return unit(Math.floor(s / HOUR), "hour");
	if (s < WEEK) return unit(Math.floor(s / DAY), "day");
	return unit(Math.floor(s / WEEK), "week");
}

/** @param {Commitment} commitment @param {number} now */
export function isSnoozed(commitment, now) {
	return typeof commitment.snoozed_until === "number" && now < commitment.snoozed_until;
}

/**
 * Open, its deadline (or window start) has passed, and it is not snoozed.
 * @param {Commitment} commitment
 * @param {number} now
 */
export function isOverdue(commitment, now) {
	const deadline = commitment.deadline;
	if (!deadline || !isOpen(commitment.status) || isSnoozed(commitment, now)) return false;
	const start = deadline.kind === "at" ? deadline.at : deadline.start;
	return now >= start;
}

/**
 * "Due in 2 hours", "Due now", "Due now, 23 hours left", "Overdue by 3 hours"; "" without a deadline.
 * @param {Commitment} commitment
 * @param {number} now
 */
export function dueLabel(commitment, now) {
	const deadline = commitment.deadline;
	if (!deadline) return "";
	if (deadline.kind === "at") {
		if (now < deadline.at) return `Due in ${describeDuration(deadline.at - now)}`;
		if (now - deadline.at < MINUTE) return "Due now";
		return `Overdue by ${describeDuration(now - deadline.at)}`;
	}
	if (now < deadline.start) return `Due in ${describeDuration(deadline.start - now)}`;
	if (now <= deadline.end) return `Due now, ${describeDuration(deadline.end - now)} left`;
	return `Overdue by ${describeDuration(now - deadline.end)}`;
}

/** The observed event as a past happening: "studio-mac connected". @param {string} event */
export function eventLabel(event) {
	const machine = event.match(/^machine_connected:(.+)$/);
	if (machine) return `${machine[1]} connected`;
	if (event.startsWith("commitment_completed:")) return "a commitment it depended on was completed";
	return event;
}

/**
 * What the commitment is held by right now: dependencies first (they block
 * before a wait is even looked at), then the waiting condition.
 * @param {Commitment} commitment
 * @param {number} now
 */
export function waitLabel(commitment, now) {
	const deps = commitment.dependencies.length;
	if (deps > 0) return `Waiting on ${deps} other commitment${deps === 1 ? "" : "s"}`;
	const wait = commitment.waiting_on;
	if (!wait) return "";
	switch (wait.kind) {
		case "until":
			return now < wait.until ? `Waiting ${describeDuration(wait.until - now)} more` : "The wait is over";
		case "event": {
			const machine = wait.event.match(/^machine_connected:(.+)$/);
			return machine ? `Waiting for ${machine[1]} to connect` : `Waiting for ${wait.event}`;
		}
		case "user_reply":
			return "Waiting for your reply";
		default:
			return "";
	}
}

/** @param {Commitment} commitment @param {number} now */
export function snoozeLabel(commitment, now) {
	if (isSnoozed(commitment, now)) {
		return `Snoozed, ${describeDuration(/** @type {number} */ (commitment.snoozed_until) - now)} left`;
	}
	const count = commitment.snooze_count;
	if (count > 0) return `Snoozed ${count} time${count === 1 ? "" : "s"}`;
	return "";
}

/** @param {Commitment} commitment @param {number} now */
export function nextCheckLabel(commitment, now) {
	if (!isOpen(commitment.status) || typeof commitment.next_check !== "number") return "";
	if (commitment.next_check <= now) return "Check pending";
	return `Next check in ${describeDuration(commitment.next_check - now)}`;
}

/** Same wording as activity receipts. @param {number} unixSeconds @param {number} nowSeconds */
function ago(unixSeconds, nowSeconds) {
	const diff = Math.max(0, nowSeconds - unixSeconds);
	if (diff < MINUTE) return "just now";
	if (diff < HOUR) return `${Math.floor(diff / MINUTE)}m ago`;
	if (diff < DAY) return `${Math.floor(diff / HOUR)}h ago`;
	return `${Math.floor(diff / DAY)}d ago`;
}

/**
 * What the last evaluation concluded, in one line, from the record only.
 * @param {CommitmentCheck | null | undefined} check
 * @param {number} now
 */
export function lastCheckLabel(check, now) {
	if (!check) return "Not checked yet";
	const parts = [`Checked ${ago(check.at, now)}`];
	const outcome = check.outcome;
	switch (outcome.kind) {
		case "triggered":
			parts.push("a check-in ran");
			break;
		case "unchanged":
			parts.push("nothing changed");
			break;
		case "failed":
			parts.push(`${outcome.retryable ? "failed and can be retried" : "failed"}: ${String(outcome.error ?? "")}`);
			break;
		case "observed":
			parts.push(`observed: ${eventLabel(String(outcome.event ?? ""))}`);
			break;
		default:
			break;
	}
	if (check.pending_event) parts.push(`still to act on: ${eventLabel(check.pending_event)}`);
	return parts.join(", ");
}

/** Fixed offsets from now, so the presets read the same in every timezone. @param {number} now */
export function snoozePresets(now) {
	return [
		{ label: "1 hour", until: now + HOUR },
		{ label: "3 hours", until: now + 3 * HOUR },
		{ label: "Tomorrow", until: now + DAY },
		{ label: "Next week", until: now + WEEK },
	];
}

/**
 * @param {number | null | undefined} until
 * @param {number} now
 * @returns {{ ok: true; until: number } | { ok: false; reason: string }}
 */
export function validateSnooze(until, now) {
	if (typeof until !== "number" || !Number.isFinite(until) || until <= now) {
		return { ok: false, reason: "Pick a time in the future." };
	}
	return { ok: true, until };
}

/** Value for a `datetime-local` input, in the browser's zone, to the minute. @param {number} unixSeconds */
export function localDateTimeValue(unixSeconds) {
	const d = new Date(unixSeconds * 1000);
	/** @param {number} n */
	const pad = (n) => String(n).padStart(2, "0");
	return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Unix seconds for a `datetime-local` value; null when empty or unparseable. @param {string} value */
export function parseLocalDateTime(value) {
	if (!value) return null;
	const ms = new Date(value).getTime();
	return Number.isFinite(ms) ? Math.floor(ms / 1000) : null;
}

/**
 * Completion is never silent: the user confirms, or says how they know.
 * The evidence is what the server records (`confirmed_by_user`, `summary`).
 * @param {{ confirmed: boolean; summary: string }} input
 * @returns {{ ok: true; evidence: { confirmed_by_user: boolean; summary?: string } } | { ok: false; reason: string }}
 */
export function completionEvidence({ confirmed, summary }) {
	const trimmed = summary.trim();
	if (trimmed.length > MAX_EVIDENCE_CHARS) {
		return { ok: false, reason: `Keep the evidence under ${MAX_EVIDENCE_CHARS} characters.` };
	}
	if (!confirmed && trimmed === "") {
		return { ok: false, reason: "Confirm it is done, or say how you know." };
	}
	/** @type {{ confirmed_by_user: boolean; summary?: string }} */
	const evidence = { confirmed_by_user: confirmed };
	if (trimmed !== "") evidence.summary = trimmed;
	return { ok: true, evidence };
}

/** @param {Commitment} commitment */
function deadlineStart(commitment) {
	const d = commitment.deadline;
	if (!d) return Number.POSITIVE_INFINITY;
	return d.kind === "at" ? d.at : d.start;
}

/** @type {Record<string, number>} */
const STATUS_RANK = { due: 0, active: 1, waiting: 2, blocked: 3 };

/**
 * Open commitments only: due first (earliest deadline first), then active,
 * waiting, blocked, each by next check (none last), then newest first.
 * @param {Commitment[]} list
 * @param {number} now
 */
export function sortOpen(list, now) {
	const open = list.filter((c) => isOpen(c.status));
	/** @param {Commitment} c */
	const rank = (c) => (isOverdue(c, now) ? 0 : (STATUS_RANK[c.status] ?? 4));
	/** @param {Commitment} c */
	const check = (c) => (typeof c.next_check === "number" ? c.next_check : Number.POSITIVE_INFINITY);
	return open.sort((a, b) => {
		const byRank = rank(a) - rank(b);
		if (byRank !== 0) return byRank;
		if (rank(a) === 0) {
			const byDeadline = deadlineStart(a) - deadlineStart(b);
			if (byDeadline !== 0) return byDeadline;
		}
		const byCheck = check(a) - check(b);
		if (byCheck !== 0) return byCheck;
		return b.created_at - a.created_at || (b.id < a.id ? -1 : 1);
	});
}

/**
 * Insert or replace a commitment, keeping newest first by creation time.
 * @param {Commitment[]} list
 * @param {Commitment} commitment
 */
export function upsertCommitment(list, commitment) {
	const next = list.filter((c) => c.id !== commitment.id);
	next.push(commitment);
	next.sort((a, b) => b.created_at - a.created_at || (b.id < a.id ? -1 : 1));
	return next;
}

const FALLBACK_REFUSAL = "Could not save that. Please try again.";

/**
 * The server answers refusals as JSON `{error, message}`; say them in the
 * user's words and never show a raw status line.
 * @param {string} text
 */
export function refusalMessage(text) {
	/** @type {{ error?: unknown; message?: unknown } | null} */
	let body = null;
	try {
		body = text ? JSON.parse(text) : null;
	} catch {
		body = null;
	}
	if (!body || typeof body !== "object") return FALLBACK_REFUSAL;
	switch (body.error) {
		case "evidence_required":
			return "Confirm it is done, or say how you know.";
		case "closed":
			return "This commitment is already closed.";
		case "not_found":
			return "This commitment no longer exists.";
		case "superseded":
			return "Something changed meanwhile. Reload and try again.";
		case "invalid":
			return typeof body.message === "string" && body.message ? body.message : FALLBACK_REFUSAL;
		default:
			return FALLBACK_REFUSAL;
	}
}
