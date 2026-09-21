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
// Requests sent to paired companions (#110): the outbox entry is the only
// source. Stubs until the labels are written.
// ---------------------------------------------------------------------------

/** @param {any} entry */
export function outboxLabel(entry) {
	void entry;
	return '';
}

/** @param {any} entry */
export function outboxText(entry) {
	void entry;
	return '';
}

/** @param {any} entry */
export function outboxStatusLabel(entry) {
	void entry;
	return '';
}

/**
 * @param {any} entry
 * @param {number} nowSeconds
 */
export function outboxNote(entry, nowSeconds) {
	void entry;
	void nowSeconds;
	return '';
}

/**
 * @param {any[]} entries
 * @param {any} entry
 */
export function upsertOutboxEntry(entries, entry) {
	void entry;
	return entries;
}
