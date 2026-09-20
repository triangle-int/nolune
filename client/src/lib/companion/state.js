// @ts-check
/**
 * Companion state (#86). Little Moon shows what Nolune is really doing, so
 * every state here is derived from a runtime event the client received:
 * the websocket opening or closing, the agent loop starting and stopping,
 * a memory recall, a tool call, a request for the user, or a permission
 * denial. Nothing is inferred from timing or mood.
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
 * active and completed only when the last one stops.
 */

/**
 * @typedef {"offline" | "blocked" | "waiting" | "failed" | "working_remote" | "working" | "recalling" | "thinking" | "listening" | "completed" | "idle"} CompanionKind
 */

/** @typedef {{ chatId: string; tool: string; summary: string; machine: string | null }} CompanionAction */
/** @typedef {{ id: string; prompt: string; target: string | null }} CompanionRequest */
/** @typedef {{ tool: string | null; summary: string; reason: string }} CompanionBlocker */

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
 *   | { type: "settle" }
 * )} CompanionEvent
 */

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
});

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
			return next(state, { listening: true, completed: false, error: null, blocker: null });
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
		case "agent_stopped": {
			const chatId = event.chatId ?? NO_CHAT;
			const seen = state.runs.includes(chatId);
			const runs = seen ? Object.freeze(state.runs.filter((id) => id !== chatId)) : state.runs;
			const last = runs.length === 0;
			const error = event.error ? event.error : state.error;
			return next(state, {
				runs,
				listening: last ? false : state.listening,
				action: state.action && state.action.chatId === chatId ? null : state.action,
				acted: last ? false : state.acted,
				recalled: last ? 0 : state.recalled,
				error,
				// Success is only claimed for a run the client saw, once the last
				// run is over, with no error, blocker or open request.
				completed: seen && last && !error && !state.blocker && !state.waiting,
			});
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
		{ kind: "working_remote", label: "Working on another computer", source: "The current tool call names another machine." },
		{ kind: "working", label: "Working locally", source: "A tool call is in progress on this computer." },
		{ kind: "recalling", label: "Recalling", source: "memory_recall arrived and no action has started yet." },
		{ kind: "thinking", label: "Thinking", source: "agent_running with no recall or action yet, or a reply after the last action." },
		{ kind: "listening", label: "Listening", source: "The server accepted your message; the run has not started." },
		{ kind: "completed", label: "Completed", source: "agent_stopped without an error, blocker or open request." },
		{ kind: "idle", label: "Idle", source: "Connected, nothing in progress." },
	]),
);

/** @param {CompanionKind} kind */
export function companionLabel(kind) {
	return COMPANION_KINDS.find((k) => k.kind === kind)?.label ?? "Idle";
}

/**
 * Accessible status text: a sentence that names the related action, machine,
 * request or blocker, so the state is understandable without motion.
 * @param {CompanionState} state
 * @param {string} [name] the companion's name; defaults to the product name.
 */
export function companionStatusText(state, name = "Nolune") {
	switch (state.kind) {
		case "offline":
			if (!state.wasConnected) return `${name} is connecting.`;
			return state.reconnecting ? `${name} is offline, reconnecting (attempt ${state.attempt}).` : `${name} is offline.`;
		case "blocked": {
			const blocker = /** @type {CompanionBlocker} */ (state.blocker);
			const what = blocker.summary || blocker.tool || "an action";
			return `${name} is blocked by permissions: ${what} (${blocker.reason}).`;
		}
		case "waiting":
			return `${name} is waiting for you: ${/** @type {CompanionRequest} */ (state.waiting).prompt}.`;
		case "failed":
			return `${name} stopped with an error: ${state.error}.`;
		case "working_remote": {
			const action = /** @type {CompanionAction} */ (state.action);
			return `${name} is working on ${action.machine}: ${action.summary}.`;
		}
		case "working":
			return `${name} is working on this computer: ${/** @type {CompanionAction} */ (state.action).summary}.`;
		case "recalling":
			return `${name} is recalling ${state.recalled} ${state.recalled === 1 ? "memory" : "memories"}.`;
		case "thinking":
			return `${name} is thinking.`;
		case "listening":
			return `${name} is listening.`;
		case "completed":
			return `${name} finished.`;
		default:
			return `${name} is idle.`;
	}
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

/**
 * Maps a websocket event onto the reducer vocabulary. Returns null for events
 * that say nothing about what the companion is doing. `machine` stays null
 * until the tool trail names the target computer (#80).
 * @param {import("../api/types.js").ServerEvent} event
 * @returns {CompanionEvent | null}
 */
export function companionEventFromServer(event) {
	switch (event.type) {
		case "chat_message_created": {
			const { message: msg, chat_id: chatId } = event;
			if (msg.kind === "tool_call") {
				return { type: "action", chatId, tool: msg.tool_name ?? "tool", summary: msg.content, machine: null };
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
			return { type: "action", chatId: event.chat_id, tool: event.tool_name, summary: event.summary, machine: null };
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
		const running = { type: "agent_running", chatId: "example" };
		/** @type {CompanionEvent} */
		const reading = { type: "action", chatId: "example", tool: "read_file", summary: "reading notes/tea.md" };
		/** @type {readonly { kind: CompanionKind; events: readonly CompanionEvent[] }[]} */
		const examples = [
			{ kind: "offline", events: [online, running, reading, { type: "connection", connected: false, reconnecting: true, attempt: 2 }] },
			{ kind: "blocked", events: [online, running, { type: "action", chatId: "example", tool: "run_command", summary: "running command" }, { type: "permission_denied", chatId: "example", reason: "ls: /root: Permission denied" }] },
			{ kind: "waiting", events: [online, running, { type: "approval_requested", id: "example", prompt: "a GitHub token for gh", target: "GITHUB_TOKEN" }] },
			{ kind: "failed", events: [online, running, reading, { type: "run_failed", chatId: "example", error: "something went wrong" }, { type: "agent_stopped", chatId: "example" }] },
			{ kind: "working_remote", events: [online, running, { type: "action", chatId: "example", tool: "computer_use", summary: "opening Finder", machine: "studio-mac" }] },
			{ kind: "working", events: [online, running, reading] },
			{ kind: "recalling", events: [online, running, { type: "memory_recall", chatId: "example", count: 3 }] },
			{ kind: "thinking", events: [online, { type: "user_message", chatId: "example" }, running] },
			{ kind: "listening", events: [online, { type: "user_message", chatId: "example" }] },
			{ kind: "completed", events: [online, running, reading, { type: "agent_stopped", chatId: "example" }] },
			{ kind: "idle", events: [online] },
		];
		return examples.map((e) => Object.freeze({ kind: e.kind, events: Object.freeze(e.events) }));
	})(),
);
