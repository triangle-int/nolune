// @ts-check
/**
 * Pure helpers for activity receipts (#94). The server's proactive run record
 * is the only source: trigger, stated reason, target, status, approvals, and
 * a receipt of actions. No model text ever reaches this module.
 */

/** @typedef {{ kind: string; [key: string]: unknown }} Tagged */
/**
 * @typedef {{
 *   version: number;
 *   id: string;
 *   dedupe_key: string;
 *   trigger: Tagged;
 *   reason: string;
 *   target: Tagged;
 *   status: Tagged;
 *   attempt: number;
 *   started_at: number;
 *   finished_at?: number;
 *   approvals: { side_effect: string; allowed: boolean; reason?: string; at: number }[];
 *   outcome?: { actions: { tool: string; summary: string }[]; messages_sent: number; tokens: number } | null;
 * }} ProactiveRun
 */

/**
 * @param {Tagged} trigger
 * @param {{ machine_id: string; display_name: string }[]} [machines] Names a connected computer (#80).
 */
export function triggerLabel(trigger, machines = []) {
	switch (trigger.kind) {
		case "heartbeat":
			return trigger.agent === "reflection" ? "Reflection" : "Check-in";
		case "schedule":
			return "Scheduled";
		case "machine_connected":
			return `Computer connected${trigger.machine_id ? ` · ${machineName(String(trigger.machine_id), machines)}` : ""}`;
		case "manual":
			return "Run by you";
		case "commitment":
			return "Commitment";
		case "handoff":
			return "Handoff";
		default:
			return "Activity";
	}
}

/**
 * Where a run acted (#80): the target machine by the name the Computers tab
 * shows, or nothing for a run that did not act on a computer.
 * @param {Tagged} target
 * @param {{ machine_id: string; display_name: string }[]} [machines]
 */
export function targetLabel(target, machines = []) {
	if (target.kind !== "machine") return "";
	return `On ${machineName(String(target.machine_id ?? ""), machines)}`;
}

/**
 * The target line an Activity card shows (#80): the run's target machine
 * unless the trigger already names that same computer ("Computer connected
 * · studio" is not followed by "On studio").
 * @param {ProactiveRun} run
 * @param {{ machine_id: string; display_name: string }[]} [machines]
 */
export function runTargetLabel(run, machines = []) {
	if (run.trigger.kind === "machine_connected" && run.trigger.machine_id === run.target.machine_id) return "";
	return targetLabel(run.target, machines);
}

/**
 * A machine by the name the Computers tab shows, else its id.
 * @param {string} machineId
 * @param {{ machine_id: string; display_name: string }[]} machines
 */
function machineName(machineId, machines) {
	return machines.find((m) => m.machine_id === machineId)?.display_name || machineId;
}

/** @param {Tagged} status */
export function statusLabel(status) {
	switch (status.kind) {
		case "running":
			return "Running";
		case "completed":
			return "Done";
		case "cancelled":
			return "Cancelled";
		case "failed":
			return status.retryable ? "Failed, can retry" : "Failed";
		case "skipped": {
			const reason = /** @type {Tagged | undefined} */ (status.reason);
			switch (reason?.kind) {
				case "quiet_hours":
					return "Held for quiet hours";
				case "cooldown":
					return "Held, too soon";
				case "duplicate":
					return "Already running";
				case "disabled":
					return "Initiative is off";
				case "import":
					return "Held for an import";
				default:
					return "Skipped";
			}
		}
		default:
			return "Unknown";
	}
}

/** @param {ProactiveRun} run */
export function canCancel(run) {
	return run.status.kind === "running";
}

/** @param {ProactiveRun} run */
export function canRetry(run) {
	return run.status.kind === "cancelled" || (run.status.kind === "failed" && run.status.retryable === true);
}

/**
 * Human summary of what a run did, from receipts only.
 * @param {ProactiveRun} run
 */
export function outcomeSummary(run) {
	const outcome = run.outcome;
	if (!outcome) return "";
	const parts = [];
	if (outcome.messages_sent > 0) parts.push(outcome.messages_sent === 1 ? "sent you a message" : `sent you ${outcome.messages_sent} messages`);
	const other = outcome.actions.filter((a) => a.tool !== "reach_out").length;
	if (other > 0) parts.push(other === 1 ? "1 action" : `${other} actions`);
	if (parts.length === 0) return "nothing to do";
	return parts.join(", ");
}

/**
 * Insert or replace a run, keeping newest first by start time.
 * @param {ProactiveRun[]} runs
 * @param {ProactiveRun} run
 */
export function upsertRun(runs, run) {
	const next = runs.filter((r) => r.id !== run.id);
	next.push(run);
	next.sort((a, b) => b.started_at - a.started_at || (b.id < a.id ? -1 : 1));
	return next;
}

/**
 * @param {number} unixSeconds
 * @param {number} nowSeconds
 */
export function relativeTime(unixSeconds, nowSeconds) {
	const diff = Math.max(0, nowSeconds - unixSeconds);
	if (diff < 60) return "just now";
	if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
	if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
	if (diff < 7 * 86400) return `${Math.floor(diff / 86400)}d ago`;
	return new Date(unixSeconds * 1000).toLocaleDateString([], { month: "short", day: "numeric" });
}

/** The trigger-condition summaries the evaluator states on a commitment run's reason (#85). */
const COMMITMENT_CONDITIONS = ["dependency completed", "event observed", "snooze ended", "deadline arrived", "wait ended", "scheduled check"];

/**
 * Link a commitment check-in back to its commitment and the exact trigger
 * condition (#85). The run's reason is `<condition>: <promise>`; only the
 * leading known condition is split off, and an unknown shape is kept whole.
 * @param {ProactiveRun} run
 * @returns {{ commitmentId: string; condition: string; promise: string } | null}
 */
export function commitmentReceipt(run) {
	if (run.trigger.kind !== "commitment") return null;
	const commitmentId = String(run.trigger.commitment_id ?? "");
	const sep = run.reason.indexOf(": ");
	const head = sep === -1 ? "" : run.reason.slice(0, sep);
	if (COMMITMENT_CONDITIONS.includes(head)) {
		return { commitmentId, condition: head, promise: run.reason.slice(sep + 2) };
	}
	return { commitmentId, condition: "", promise: run.reason };
}

/** What changed and why the companion looked now, in the user's words. @param {string} condition */
export function commitmentConditionLabel(condition) {
	switch (condition) {
		case "deadline arrived":
			return "Its deadline arrived";
		case "snooze ended":
			return "The snooze you asked for ended";
		case "wait ended":
			return "The wait it was on ended";
		case "event observed":
			return "Something it was waiting for happened";
		case "dependency completed":
			return "A commitment it depended on was completed";
		case "scheduled check":
			return "A scheduled check came up; nothing else changed";
		default:
			return "It looked at this commitment";
	}
}

// ---------------------------------------------------------------------------
// Requests sent to paired companions (#110). The server's outbox entry is
// the only source: the intent this companion sent (its owner's own words),
// where it stands, every attempt, and the peer's typed response, which
// carries ids, classes, times, and spans and never a word of the peer's.
// ---------------------------------------------------------------------------

/** @typedef {import("../api/types.js").FederationOutboxEntry} OutboxEntry */

/**
 * "TFccHElq…cQ7E": a companion id shortened the way the Companions section
 * shows it.
 * @param {string} id
 */
function shortCompanionId(id) {
	return id.length <= 14 ? id : `${id.slice(0, 8)}…${id.slice(-4)}`;
}

/**
 * "in 4m", "in 30s", or "now" for a moment ahead of the clock.
 * @param {number} unixSeconds
 * @param {number} nowSeconds
 */
function untilLabel(unixSeconds, nowSeconds) {
	const diff = unixSeconds - nowSeconds;
	if (diff <= 0) return "now";
	if (diff < 60) return `in ${diff}s`;
	if (diff < 3600) return `in ${Math.floor(diff / 60)}m`;
	return `in ${Math.floor(diff / 3600)}h`;
}

/** @param {number} unixSeconds */
function momentLabel(unixSeconds) {
	return new Date(unixSeconds * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/**
 * What was asked of whom: the request kind and the companion, never the
 * words.
 * @param {OutboxEntry} entry
 */
export function outboxLabel(entry) {
	const peer = shortCompanionId(entry.recipient);
	switch (entry.intent.intent.type) {
		case "message":
			return `Message to companion ${peer}`;
		case "availability":
			return `Availability asked of companion ${peer}`;
		case "reminder":
			return `Reminder proposed to companion ${peer}`;
		default:
			return `Request to companion ${peer}`;
	}
}

/**
 * This owner's own words as sent, or the window an availability query
 * asked about; nothing for a kind this page does not know.
 * @param {OutboxEntry} entry
 */
export function outboxText(entry) {
	const payload = entry.intent.intent;
	switch (payload.type) {
		case "message":
			return String(payload.body ?? "");
		case "reminder":
			return String(payload.text ?? "");
		case "availability": {
			const window = /** @type {{ from: number; to: number } | undefined} */ (payload.window);
			if (!window) return "";
			return `Free between ${momentLabel(window.from)} and ${momentLabel(window.to)}?`;
		}
		default:
			return "";
	}
}

/**
 * One word on where the request stands.
 * @param {OutboxEntry} entry
 */
export function outboxStatusLabel(entry) {
	switch (entry.status) {
		case "queued":
			return entry.attempts.length === 0 ? "Queued" : "Not delivered yet";
		case "waiting_owner":
			return "Waiting for their owner";
		case "delivered":
			return "Delivered";
		case "denied":
			return "Refused";
		case "failed":
			return "Failed";
		case "expired":
			return "Expired";
		default:
			return "Unknown";
	}
}

/**
 * Whether a refused attempt was answered by something in front of the
 * companion (a proxy's or a tunnel's own error page) rather than by its
 * route: the body named no code, and the status is on record.
 * @param {{ status?: number; code: string }} refusal
 */
function answeredInFront(refusal) {
	return refusal.code === "unknown" && typeof refusal.status === "number";
}

/**
 * Why the last attempt did not deliver, from its recorded outcome.
 * @param {OutboxEntry} entry
 */
function lastFailure(entry) {
	const last = entry.attempts[entry.attempts.length - 1];
	if (!last) return "Could not reach it";
	switch (last.outcome.kind) {
		case "refused":
			return answeredInFront(last.outcome) ? `Something in front of it answered HTTP ${last.outcome.status}` : `Their companion refused for now (${last.outcome.code})`;
		case "answered":
			return last.outcome.reason ? `Their companion refused for now (${last.outcome.reason})` : "Their companion answered";
		default:
			return "Could not reach it";
	}
}

/** @param {number} count */
function attempts(count) {
	return count === 1 ? "1 attempt" : `${count} attempts`;
}

/** @param {number} count @param {string} noun */
function spans(count, noun) {
	return `${count} ${noun} ${count === 1 ? "span" : "spans"}`;
}

/**
 * What happened, how often it was tried, and what comes next, in one
 * sentence, from the record alone.
 * @param {OutboxEntry} entry
 * @param {number} nowSeconds
 */
export function outboxNote(entry, nowSeconds) {
	const ago = relativeTime(entry.updated_at, nowSeconds);
	const next = entry.next_attempt_at == null ? "now" : untilLabel(entry.next_attempt_at, nowSeconds);
	const count = entry.attempts.length;
	const response = entry.response;
	switch (entry.status) {
		case "queued":
			if (count === 0) return "Sending now.";
			return `${lastFailure(entry)}, ${attempts(count)} · next try ${next}.`;
		case "waiting_owner":
			return response?.outcome === "needs_owner" && response.reason === "quiet_hours" ? `Held during their quiet hours · asks again ${next}.` : `Their owner has to allow it first · asks again ${next}.`;
		case "delivered": {
			const answer = response?.outcome === "accepted" ? response.answer : undefined;
			switch (answer?.kind) {
				case "reminder_scheduled":
					return `Reminder set for ${momentLabel(Number(answer.at))} · ${ago}.`;
				case "availability": {
					const windows = answer.windows ?? [];
					if (windows.length === 0) return `Answered ${ago}: nothing about their schedule was shared.`;
					const free = windows.filter((w) => w.state === "free").length;
					return `Answered ${ago}: ${spans(free, "free")}, ${spans(windows.length - free, "busy")}.`;
				}
				case "proposal_received":
					return `Delivered to their owner ${ago}.`;
				default:
					return `Delivered to their conversation ${ago}.`;
			}
		}
		case "denied": {
			const reason = response?.outcome === "denied" ? response.reason : "";
			switch (reason) {
				case "rule":
					return `Refused by a rule their owner set · ${ago}.`;
				case "default":
					return `Refused by their defaults · ${ago}.`;
				case "owner_denied":
					return `Refused by their owner · ${ago}.`;
				default:
					return `Refused${reason ? ` (${reason})` : ""} · ${ago}.`;
			}
		}
		case "failed": {
			const last = entry.attempts[count - 1];
			if (last?.outcome.kind === "refused" && answeredInFront(last.outcome)) return `Could not reach it after ${attempts(count)}, the last answered by HTTP ${last.outcome.status} from in front of it; nothing was delivered · ${ago}.`;
			if (last?.outcome.kind === "refused") return `Their companion refused it (${last.outcome.code}); nothing was delivered · ${ago}.`;
			if (last?.outcome.kind === "answered" && last.outcome.reason === "rate_limited") return `Their companion was over its limit for ${attempts(count)} in a row; nothing was delivered · ${ago}.`;
			return `Could not reach it after ${attempts(count)}; nothing was delivered · ${ago}.`;
		}
		case "expired":
			if (response?.outcome === "needs_owner") return response.reason === "quiet_hours" ? `Expired while held during their quiet hours, ${attempts(count)} · ${ago}.` : `Expired before their owner allowed it, ${attempts(count)} · ${ago}.`;
			if (response?.outcome === "denied" && response.reason === "rate_limited") return `Expired while their companion was over its limit, ${attempts(count)} · ${ago}.`;
			return `Expired before it could be delivered, ${attempts(count)} · ${ago}.`;
		default:
			return "";
	}
}

/**
 * Insert or replace an entry by its request id, keeping the most recently
 * changed first.
 * @param {OutboxEntry[]} entries
 * @param {OutboxEntry} entry
 */
export function upsertOutboxEntry(entries, entry) {
	const next = entries.filter((e) => e.intent.correlation_id !== entry.intent.correlation_id);
	next.push(entry);
	next.sort((a, b) => b.updated_at - a.updated_at || (b.intent.correlation_id < a.intent.correlation_id ? -1 : 1));
	return next;
}
