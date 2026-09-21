// @ts-check
/**
 * Companion state (#86). Little Moon shows what Nolune is really doing, so
 * every state here is derived from a runtime event the client received:
 * the websocket opening or closing, the agent loop starting and stopping,
 * a memory recall, a tool call, a request for the user, a permission
 * denial, a proactive run's receipt, or the persisted `agent_running` of a
 * conversation snapshot. Nothing is inferred from timing or mood.
 *
 * The reducer is pure: `reduceCompanion(state, event)` returns a new frozen
 * state (or the same object when the event changes nothing), so the same
 * events always produce the same state and the UI cannot animate a state the
 * runtime has not reached.
 *
 * Priority when several facts hold at once (highest first):
 *
 * 1. offline — the socket is closed. Beats everything; the facts underneath
 *    are kept so reconnecting restores them.
 * 2. blocked — a permission or policy denial was reported in the current
 *    run. Beats working, outlasts agent_stopped (a blocked run is never shown
 *    as completed) and clears only when the next message or run starts.
 * 3. waiting — the companion asked the user for something (a secret, an
 *    approval) and has no answer. Beats working; only the answer clears it,
 *    even after the run stops.
 * 4. failed — the run reported an error: the server's `[system] <label>`
 *    assistant message (agent_stopped carries no error field), or an
 *    agent_stopped that names one. Recorded before the stop arrives, so the
 *    stop cannot turn it into completed. Never settles into idle on its own;
 *    the next message or run clears it.
 * 5. working_remote / working — a tool call is in progress, on another
 *    computer when the event names one, otherwise on this one. A tool call,
 *    a recall and agent_running all prove the run is active.
 * 6. recalling — memories were recalled in this run and no action has
 *    started yet. Recalling never overrides working: once an action runs
 *    (`acted`), a later recall only adds to the count and the phase between
 *    actions is thinking, not a stale recall.
 * 7. thinking — the run is active with no recall or action yet, or the model
 *    replied after its last action.
 * 8. listening — the server accepted the user's message and no run has
 *    started. A message to a companion that is already working is heard
 *    without interrupting the work.
 * 9. completed — the last run stopped without an error, blocker or open
 *    request, and only for a run the client saw. `settle` returns it to idle.
 * 10. idle — connected, nothing else.
 *
 * One companion can run several chats; it is working while any run is
 * active and completed only when the last one stops. A proactive run
 * (`activity_updated`, #94) is a run like any other, keyed `run:<id>`, and
 * ends the way its receipt says: completed, failed with its error, or
 * cancelled (over, nothing claimed). A conversation snapshot (`agent_running`
 * from GET /chat) starts a run the client missed and ends one that stopped
 * while the client was away, without claiming success for it.
 */

import { runTargetLabel, triggerLabel } from "../activity/receipts.js";
import { handoffAnchor } from "../continuity/resume.js";

/**
 * @typedef {"offline" | "blocked" | "waiting" | "failed" | "working_remote" | "working" | "recalling" | "thinking" | "listening" | "completed" | "idle"} CompanionKind
 */

/** @typedef {{ chatId: string; tool: string; summary: string; machine: string | null }} CompanionAction */
/** @typedef {{ id: string; prompt: string; target: string | null }} CompanionRequest */
/** @typedef {{ tool: string | null; summary: string; reason: string }} CompanionBlocker */
/**
 * The proactive run the companion is on, or the last one when no run is
 * active (kept so the finished state still links to its Activity entry).
 * @typedef {{ id: string; label: string; machine: string | null; handoffId: string | null }} CompanionActivity
 */

/**
 * @typedef {{
 *   kind: CompanionKind;
 *   connected: boolean;
 *   wasConnected: boolean;
 *   reconnecting: boolean;
 *   attempt: number;
 *   listening: boolean;
 *   runs: readonly string[];
 *   action: CompanionAction | null;
 *   acted: boolean;
 *   recalled: number;
 *   waiting: CompanionRequest | null;
 *   blocker: CompanionBlocker | null;
 *   error: string | null;
 *   completed: boolean;
 *   activity: CompanionActivity | null;
 * }} CompanionState
 */

/**
 * @typedef {(
 *   | { type: "connection"; connected: boolean; reconnecting?: boolean; attempt?: number }
 *   | { type: "user_message"; chatId?: string }
 *   | { type: "agent_running"; chatId?: string }
 *   | { type: "agent_stopped"; chatId?: string; error?: string | null }
 *   | { type: "run_failed"; chatId?: string; error: string }
 *   | { type: "memory_recall"; chatId?: string; count: number }
 *   | { type: "action"; chatId?: string; tool: string; summary: string; machine?: string | null }
 *   | { type: "assistant_message"; chatId?: string }
 *   | { type: "approval_requested"; id: string; prompt: string; target?: string | null }
 *   | { type: "approval_resolved"; id: string }
 *   | { type: "permission_denied"; chatId?: string; tool?: string; summary?: string; reason: string }
 *   | { type: "activity_run"; id: string; status: ActivityStatus; label: string; machine?: string | null; handoffId?: string | null; error?: string | null }
 *   | { type: "snapshot"; chatId?: string; running: boolean }
 *   | { type: "settle" }
 * )} CompanionEvent
 */

/** A proactive run's status as its receipt reports it (`RunStatus` on the server). */
/** @typedef {"running" | "completed" | "failed" | "cancelled" | "skipped"} ActivityStatus */

/** Chat id used for events that do not carry one. */
const NO_CHAT = "";

/** @type {Readonly<Omit<CompanionState, "kind">>} */
const INITIAL_FACTS = Object.freeze({
	connected: false,
	wasConnected: false,
	reconnecting: false,
	attempt: 0,
	listening: false,
	runs: Object.freeze([]),
	action: null,
	acted: false,
	recalled: 0,
	waiting: null,
	blocker: null,
	error: null,
	completed: false,
	activity: null,
});

/** Run id of a proactive run in `runs`, beside the chat ids. */
const activityRunId = (/** @type {string} */ id) => `run:${id}`;

/** The state before any event: the socket has not reported it is open. */
export function initialCompanionState() {
	return withKind(INITIAL_FACTS);
}

/**
 * The single place that turns facts into the displayed state, in priority
 * order (see the module comment).
 * @param {Omit<CompanionState, "kind">} f
 * @returns {CompanionKind}
 */
function resolveKind(f) {
	if (!f.connected) return "offline";
	if (f.blocker) return "blocked";
	if (f.waiting) return "waiting";
	if (f.error) return "failed";
	if (f.action) return f.action.machine ? "working_remote" : "working";
	const running = f.runs.length > 0;
	if (running && f.recalled > 0 && !f.acted) return "recalling";
	if (running) return "thinking";
	if (f.listening) return "listening";
	if (f.completed) return "completed";
	return "idle";
}

/**
 * @param {Omit<CompanionState, "kind">} facts
 * @returns {CompanionState}
 */
function withKind(facts) {
	return Object.freeze({ ...facts, kind: resolveKind(facts) });
}

/**
 * Applies `patch` when it changes something; otherwise returns `state` itself
 * so callers can skip re-rendering.
 * @param {CompanionState} state
 * @param {Partial<Omit<CompanionState, "kind">>} patch
 */
function next(state, patch) {
	let changed = false;
	for (const key of /** @type {(keyof typeof patch)[]} */ (Object.keys(patch))) {
		if (!Object.is(patch[key], state[key])) {
			changed = true;
			break;
		}
	}
	if (!changed) return state;
	const { kind: _kind, ...facts } = state;
	return withKind({ ...facts, ...patch });
}

/**
 * @param {CompanionState} state
 * @param {string} chatId
 */
function startRun(state, chatId) {
	if (state.runs.includes(chatId)) return state;
	// A new run starts clean: whatever ended the previous one is over. Facts
	// gathered inside an active run (a blocker, for instance) stay.
	return next(state, {
		runs: Object.freeze([...state.runs, chatId]),
		listening: false,
		completed: false,
		error: null,
		blocker: null,
		// The proactive run kept for the finished state's link is over.
		activity: state.runs.length === 0 ? null : state.activity,
	});
}

/**
 * Ends a run. `claim` says whether success may be claimed for it: true for
 * the runtime's own stop (agent_stopped, a completed receipt), false when the
 * run is merely known to be over (a snapshot, a cancelled receipt). `error`
 * is recorded either way, so a failure reported for a run the client never
 * saw start still shows.
 * @param {CompanionState} state
 * @param {string} runId
 * @param {boolean} claim
 * @param {string | null} error
 */
function stopRun(state, runId, claim, error) {
	const seen = state.runs.includes(runId);
	const runs = seen ? Object.freeze(state.runs.filter((id) => id !== runId)) : state.runs;
	const last = runs.length === 0;
	const nextError = error ? error : state.error;
	return next(state, {
		runs,
		listening: last ? false : state.listening,
		action: state.action && state.action.chatId === runId ? null : state.action,
		acted: last ? false : state.acted,
		recalled: last ? 0 : state.recalled,
		error: nextError,
		// Success is only claimed for a run the client saw, once the last
		// run is over, with no error, blocker or open request.
		completed: claim && seen && last && !nextError && !state.blocker && !state.waiting,
	});
}

/**
 * @param {CompanionState} state
 * @param {CompanionEvent} event
 * @returns {CompanionState}
 */
export function reduceCompanion(state, event) {
	switch (event.type) {
		case "connection":
			return next(state, {
				connected: event.connected,
				wasConnected: state.wasConnected || event.connected,
				reconnecting: !event.connected && (event.reconnecting ?? false),
				attempt: event.connected ? 0 : (event.attempt ?? 0),
			});
		case "user_message":
			return next(state, {
				listening: true,
				completed: false,
				error: null,
				blocker: null,
				activity: state.runs.length === 0 ? null : state.activity,
			});
		case "agent_running":
			return startRun(state, event.chatId ?? NO_CHAT);
		case "memory_recall": {
			const started = startRun(state, event.chatId ?? NO_CHAT);
			return next(started, { recalled: started.recalled + Math.max(0, event.count) });
		}
		case "action": {
			const chatId = event.chatId ?? NO_CHAT;
			const started = startRun(state, chatId);
			return next(started, {
				action: Object.freeze({ chatId, tool: event.tool, summary: event.summary, machine: event.machine || null }),
				acted: true,
			});
		}
		case "assistant_message": {
			const chatId = event.chatId ?? NO_CHAT;
			if (!state.action || state.action.chatId !== chatId) return state;
			return next(state, { action: null });
		}
		case "run_failed": {
			// The turn is over: its action ended with the error, and the stop that
			// follows must not claim success.
			const chatId = event.chatId ?? NO_CHAT;
			return next(state, {
				error: event.error,
				action: state.action && state.action.chatId === chatId ? null : state.action,
				completed: false,
			});
		}
		case "agent_stopped":
			return stopRun(state, event.chatId ?? NO_CHAT, true, event.error ?? null);
		case "snapshot": {
			// Persisted state, not a live event: it only starts a run the client
			// missed or ends one that stopped while the client was away.
			const chatId = event.chatId ?? NO_CHAT;
			if (event.running) return startRun(state, chatId);
			if (!state.runs.includes(chatId)) return state;
			return stopRun(state, chatId, false, null);
		}
		case "activity_run": {
			const runId = activityRunId(event.id);
			const activity = Object.freeze({
				id: event.id,
				label: event.label,
				machine: event.machine ?? null,
				handoffId: event.handoffId ?? null,
			});
			if (event.status === "running") {
				return next(startRun(state, runId), { activity });
			}
			if (event.status === "skipped") return state;
			const failed = event.status === "failed" ? event.error || "the run failed" : null;
			if (!state.runs.includes(runId) && !failed) return state;
			const stopped = stopRun(state, runId, event.status === "completed", failed);
			// The finished run stays linked while nothing else runs; while other
			// runs continue it is no longer what the companion is on.
			const last = stopped.runs.length === 0;
			const current = state.activity && state.activity.id === event.id;
			return next(stopped, { activity: last ? activity : current ? null : state.activity });
		}
		case "approval_requested":
			return next(state, {
				waiting: Object.freeze({ id: event.id, prompt: event.prompt, target: event.target ?? null }),
			});
		case "approval_resolved":
			if (!state.waiting || state.waiting.id !== event.id) return state;
			return next(state, { waiting: null });
		case "permission_denied": {
			const related = state.action && state.action.chatId === (event.chatId ?? NO_CHAT) ? state.action : null;
			return next(state, {
				blocker: Object.freeze({
					tool: event.tool ?? related?.tool ?? null,
					summary: event.summary ?? related?.summary ?? "",
					reason: event.reason,
				}),
			});
		}
		case "settle":
			return next(state, { completed: false });
		default:
			return state;
	}
}

/** Human labels in priority order; also the order of the design-system gallery. */
export const COMPANION_KINDS = Object.freeze(
	/** @type {readonly { kind: CompanionKind; label: string; source: string }[]} */ ([
		{ kind: "offline", label: "Offline", source: "The websocket closed or has not opened yet." },
		{ kind: "blocked", label: "Blocked by permissions", source: "A tool's output reported a permission or policy denial in this run." },
		{ kind: "waiting", label: "Waiting for approval", source: "A secret_request or approval is open and unanswered." },
		{ kind: "failed", label: "Failed", source: "The server reported the run failed ([system] line) before agent_stopped." },
		{ kind: "working_remote", label: "Working on another computer", source: "The current tool call's trail line names another computer (#80)." },
		{ kind: "working", label: "Working locally", source: "A tool call is in progress on this computer." },
		{ kind: "recalling", label: "Recalling", source: "memory_recall arrived and no action has started yet." },
		{ kind: "thinking", label: "Thinking", source: "agent_running or a proactive run with no recall or action yet, or a reply after the last action." },
		{ kind: "listening", label: "Listening", source: "The server accepted your message; the run has not started." },
		{ kind: "completed", label: "Completed", source: "agent_stopped or a completed receipt, without an error, blocker or open request." },
		{ kind: "idle", label: "Idle", source: "Connected, nothing in progress." },
	]),
);

/** @param {CompanionKind} kind */
export function companionLabel(kind) {
	return COMPANION_KINDS.find((k) => k.kind === kind)?.label ?? "Idle";
}

/** What the status sentence's focus links to. @typedef {"machine" | "chat" | "run" | "handoff" | null} StatusLink */

/**
 * The status sentence in three parts: `lead + focus + tail` is the sentence
 * and `focus` is the phrase that names the related action, machine, request
 * or blocker, with `link` saying where that phrase leads.
 * @param {CompanionState} state
 * @param {string} name
 * @returns {{ lead: string; focus: string; tail: string; link: StatusLink }}
 */
function statusParts(state, name) {
	/** @param {string} lead @param {string} focus @param {string} tail @param {StatusLink} link */
	const parts = (lead, focus = "", tail = "", link = null) => ({ lead, focus, tail, link });
	const activity = state.activity;
	/** The link for a state about the run as a whole: the proactive run when one is (or was) it, else the conversation. */
	const runLink = () => (activity ? (activity.handoffId ? "handoff" : "run") : "chat");
	switch (state.kind) {
		case "offline":
			if (!state.wasConnected) return parts(`${name} is connecting.`);
			return parts(state.reconnecting ? `${name} is offline, reconnecting (attempt ${state.attempt}).` : `${name} is offline.`);
		case "blocked": {
			const blocker = /** @type {CompanionBlocker} */ (state.blocker);
			const what = blocker.summary || blocker.tool || "an action";
			return parts(`${name} is blocked by permissions: `, what, ` (${blocker.reason}).`, "chat");
		}
		case "waiting":
			return parts(`${name} is waiting for you: ${/** @type {CompanionRequest} */ (state.waiting).prompt}.`);
		case "failed":
			return parts(`${name} stopped with an error: `, /** @type {string} */ (state.error), ".", runLink());
		case "working_remote": {
			const action = /** @type {CompanionAction} */ (state.action);
			return parts(`${name} is working on `, /** @type {string} */ (action.machine), `: ${action.summary}.`, "machine");
		}
		case "working":
			return parts(`${name} is working on this computer: `, /** @type {CompanionAction} */ (state.action).summary, ".", "chat");
		case "recalling":
			return parts(`${name} is recalling ${state.recalled} ${state.recalled === 1 ? "memory" : "memories"}.`);
		case "thinking":
			return activity ? parts(`${name} is thinking: `, activity.label, ".", runLink()) : parts(`${name} is thinking.`);
		case "listening":
			return parts(`${name} is listening.`);
		case "completed":
			return activity ? parts(`${name} finished: `, activity.label, ".", runLink()) : parts(`${name} finished.`);
		default:
			return parts(`${name} is idle.`);
	}
}

/**
 * Accessible status text: a sentence that names the related action, machine,
 * request or blocker, so the state is understandable without motion.
 * @param {CompanionState} state
 * @param {string} [name] the companion's name; defaults to the product name.
 */
export function companionStatusText(state, name = "Nolune") {
	const { lead, focus, tail } = statusParts(state, name);
	return lead + focus + tail;
}

/**
 * Where the status sentence's focus leads, on the companion's routes: the
 * Computers tab for a machine, the conversation an action or blocker
 * belongs to, the Activity entry of a proactive run, or its handoff card.
 * @param {CompanionState} state
 * @param {StatusLink} link
 * @param {string} slug
 */
function statusHref(state, link, slug) {
	switch (link) {
		case "machine":
			return `/${slug}/computers`;
		case "chat": {
			const chatId = state.action?.chatId || state.runs.find((id) => !id.startsWith("run:")) || "";
			return chatId && chatId !== "default" ? `/${slug}/chat/${chatId}` : `/${slug}/chat`;
		}
		case "run":
			return `/${slug}/activity#run-${/** @type {CompanionActivity} */ (state.activity).id}`;
		case "handoff":
			return `/${slug}/activity#${handoffAnchor(/** @type {string} */ (/** @type {CompanionActivity} */ (state.activity).handoffId))}`;
		default:
			return null;
	}
}

/**
 * The status sentence as segments, the focus carrying a link to the related
 * machine, conversation, Activity run or handoff card when the companion's
 * `slug` is known. Joined, the segments read exactly as `companionStatusText`.
 * @param {CompanionState} state
 * @param {string} [name]
 * @param {string | null} [slug]
 * @returns {readonly ({ text: string } | { text: string; href: string })[]}
 */
export function companionStatusSegments(state, name = "Nolune", slug = null) {
	const { lead, focus, tail, link } = statusParts(state, name);
	const href = slug && focus ? statusHref(state, link, slug) : null;
	if (!href) return Object.freeze([Object.freeze({ text: lead + focus + tail })]);
	return Object.freeze([Object.freeze({ text: lead }), Object.freeze({ text: focus, href }), Object.freeze({ text: tail })]);
}

/** An errno code for a permission denial; never prose, so a diagnostic line carrying it is a denial. */
const ERRNO_DENIAL = /\bEACCES\b|\bEPERM\b|\[Errno (?:1|13)\]/;
/** What an OS or runtime says when it refuses an action for lack of permission. */
const DENIAL = new RegExp(`permission denied|operation not permitted|${ERRNO_DENIAL.source}`, "i");
/** A tool-level refusal; only trusted on an explicit `error:` line, where the text is the tool's own. */
const POLICY_DENIAL = /not permitted|not allowed/i;
/** A diagnostic line: `<program or path>: …`. */
const DIAGNOSTIC = /^[^\s:][^:]*:\s/;
/**
 * A raw diagnostic line whose denial phrase either ends the line
 * (`ls: /root: Permission denied`, `…: Permission denied (publickey).`) or
 * follows the program name (`zsh: permission denied: ./x`). Prose that
 * merely mentions permissions has neither shape.
 */
const RAW_DENIAL = /^[^\s:][^:]*:\s+(?:(?:permission denied|operation not permitted)\b|(?:.*:\s*)?(?:permission denied|operation not permitted)\b(?:\s*\([^)]*\))?[.!]?\s*$)/i;

/**
 * The reason a tool output reports a permission denial, or null.
 * `run_command` (with `interactive_session` the only tool whose output is
 * broadcast live) has three shapes: the tool itself failed (`error:
 * <reason>`); a non-PTY run returned a non-zero exit as `stdout…\nstderr:
 * <text>`; a PTY run (the default) returned the raw terminal text with no
 * marker at all. Ordinary output is never a blocker: outside an `error:` or
 * `stderr:` marker a line must look like a diagnostic.
 * @param {string} text
 */
export function permissionDenial(text) {
	const trimmed = text.trim();
	const error = /^error:\s*(.+)$/s.exec(trimmed);
	if (error) {
		const reason = error[1].trim();
		return DENIAL.test(reason) || POLICY_DENIAL.test(reason) ? reason : null;
	}
	const lines = trimmed.split(/\r?\n/).map((line) => line.trim());
	const stderrAt = lines.findIndex((line) => /^stderr:/.test(line));
	if (stderrAt >= 0) {
		const stderr = lines.slice(stderrAt);
		stderr[0] = stderr[0].replace(/^stderr:\s*/, "");
		return stderr.find((line) => line && DENIAL.test(line)) ?? null;
	}
	return lines.find((line) => RAW_DENIAL.test(line) || (DIAGNOSTIC.test(line) && ERRNO_DENIAL.test(line))) ?? null;
}

/** Status lines the server writes as `[system] …` assistant messages; none is a run's outcome. */
const SYSTEM_STATUS = /^(?:mood →|rhythm update|routine '.*' ran|desktop '.*' connected|user left this instance)/;

/**
 * Classifies a `[system]` assistant message. The server has no error field on
 * agent_stopped: a failed turn is reported as `[system] <error label>`
 * (`something went wrong`, `no API key configured — …`, `request timed
 * out`, …) right before the stop. Returns that text for a failure, "" for a
 * status line, or null when the content is not a system line at all.
 * @param {string} content
 */
export function systemFailure(content) {
	const match = /^\[system\]\s*([\s\S]*)$/.exec(content.trim());
	if (!match) return null;
	const text = match[1].trim();
	return SYSTEM_STATUS.test(text) ? "" : text;
}

/** The tools that act on another computer, whose trail line names it (#80). */
const MACHINE_TOOLS = new Set(["computer_use", "remote_bash", "remote_files"]);
/** How the trail describes the server's own computer: the desktop tools refused, nothing acted elsewhere. */
const SERVER_HOME = "the server home";

/**
 * The computer a tool call acts on, from the trail line the tool announced
 * (#80): `<action> on <computer>`, the computer named the way the Computers
 * tab shows it (else its id). Only the desktop tools name one; the server
 * home is this computer; a line recorded before #80 names nothing.
 * @param {string} tool
 * @param {string} summary
 * @returns {{ summary: string; machine: string | null }}
 */
export function machineFromTrail(tool, summary) {
	if (!MACHINE_TOOLS.has(tool)) return { summary, machine: null };
	const at = summary.lastIndexOf(" on ");
	if (at < 0) return { summary, machine: null };
	const machine = summary.slice(at + 4).trim();
	const action = summary.slice(0, at).trim();
	if (!machine || !action) return { summary, machine: null };
	return { summary: action, machine: machine === SERVER_HOME ? null : machine };
}

/**
 * A `tool_call` message or `tool_activity` event as a reducer action.
 * @param {string} chatId
 * @param {string} tool
 * @param {string} summary
 * @returns {CompanionEvent}
 */
function actionEvent(chatId, tool, summary) {
	const trail = machineFromTrail(tool, summary);
	return { type: "action", chatId, tool, summary: trail.summary, machine: trail.machine };
}

/**
 * A proactive run's receipt as a reducer event (#94). The label is the
 * trigger the Activity view shows, with the target computer when the run
 * acts on one, named the way the Computers tab does when `machines` has it.
 * @param {import("../api/types.js").ProactiveRun} run
 * @param {{ machine_id: string; display_name: string }[]} machines
 * @returns {CompanionEvent}
 */
function activityEvent(run, machines) {
	const target = runTargetLabel(run, machines);
	const machineId = run.target.kind === "machine" ? String(run.target.machine_id ?? "") : "";
	const machine = machineId ? machines.find((m) => m.machine_id === machineId)?.display_name || machineId : null;
	const status = /** @type {ActivityStatus} */ (run.status.kind);
	return {
		type: "activity_run",
		id: run.id,
		status,
		label: target ? `${triggerLabel(run.trigger, machines)} ${target[0].toLowerCase()}${target.slice(1)}` : triggerLabel(run.trigger, machines),
		machine,
		handoffId: run.trigger.kind === "handoff" ? String(run.trigger.handoff_id ?? "") || null : null,
		error: status === "failed" ? String(run.status.error ?? "") || null : null,
	};
}

/**
 * Maps a websocket event onto the reducer vocabulary. Returns null for events
 * that say nothing about what the companion is doing. A desktop tool's
 * `machine` comes from its trail line (#80); `machines` names a proactive
 * run's target computer the way the Computers tab does.
 * @param {import("../api/types.js").ServerEvent} event
 * @param {{ machine_id: string; display_name: string }[]} [machines]
 * @returns {CompanionEvent | null}
 */
export function companionEventFromServer(event, machines = []) {
	switch (event.type) {
		case "chat_message_created": {
			const { message: msg, chat_id: chatId } = event;
			if (msg.kind === "tool_call") {
				return actionEvent(chatId, msg.tool_name ?? "tool", msg.content);
			}
			if (msg.kind === "tool_output") {
				const reason = permissionDenial(msg.content);
				return reason ? { type: "permission_denied", chatId, tool: msg.tool_name ?? "tool", reason } : null;
			}
			if (msg.kind && msg.kind !== "message") return null;
			if (msg.role === "user") return { type: "user_message", chatId };
			const failure = systemFailure(msg.content);
			if (failure === null) return { type: "assistant_message", chatId };
			return failure ? { type: "run_failed", chatId, error: failure } : null;
		}
		case "tool_activity":
			return actionEvent(event.chat_id, event.tool_name, event.summary);
		case "activity_updated":
			return activityEvent(event.run, machines);
		case "agent_running":
			return { type: "agent_running", chatId: event.chat_id };
		case "agent_stopped":
			return { type: "agent_stopped", chatId: event.chat_id };
		case "memory_recall": {
			// The server sends chat_id; the client type does not declare it yet.
			const chatId = "chat_id" in event && typeof event.chat_id === "string" ? event.chat_id : undefined;
			return { type: "memory_recall", chatId, count: event.memories.length };
		}
		case "secret_request":
			return { type: "approval_requested", id: event.id, prompt: event.prompt, target: event.target };
		default:
			return null;
	}
}

/**
 * One event sequence per state, reduced live by the design-system gallery
 * so the examples cannot drift from the reducer.
 * @type {readonly { kind: CompanionKind; events: readonly CompanionEvent[] }[]}
 */
export const STATE_EXAMPLES = Object.freeze(
	(() => {
		/** @type {CompanionEvent} */
		const online = { type: "connection", connected: true };
		/** @type {CompanionEvent} */
		const running = { type: "agent_running", chatId: "default" };
		/** @type {CompanionEvent} */
		const reading = { type: "action", chatId: "default", tool: "read_file", summary: "reading notes/tea.md" };
		/** @type {readonly { kind: CompanionKind; events: readonly CompanionEvent[] }[]} */
		const examples = [
			{ kind: "offline", events: [online, running, reading, { type: "connection", connected: false, reconnecting: true, attempt: 2 }] },
			{ kind: "blocked", events: [online, running, { type: "action", chatId: "default", tool: "run_command", summary: "running command" }, { type: "permission_denied", chatId: "default", reason: "ls: /root: Permission denied" }] },
			{ kind: "waiting", events: [online, running, { type: "approval_requested", id: "example", prompt: "a GitHub token for gh", target: "GITHUB_TOKEN" }] },
			{ kind: "failed", events: [online, running, reading, { type: "run_failed", chatId: "default", error: "something went wrong" }, { type: "agent_stopped", chatId: "default" }] },
			{ kind: "working_remote", events: [online, running, { type: "action", chatId: "default", tool: "computer_use", summary: "opening Finder", machine: "studio-mac" }] },
			{ kind: "working", events: [online, running, reading] },
			{ kind: "recalling", events: [online, running, { type: "memory_recall", chatId: "default", count: 3 }] },
			{ kind: "thinking", events: [online, { type: "user_message", chatId: "default" }, running] },
			{ kind: "listening", events: [online, { type: "user_message", chatId: "default" }] },
			{ kind: "completed", events: [online, running, reading, { type: "agent_stopped", chatId: "default" }] },
			{ kind: "idle", events: [online] },
		];
		return examples.map((e) => Object.freeze({ kind: e.kind, events: Object.freeze(e.events) }));
	})(),
);
