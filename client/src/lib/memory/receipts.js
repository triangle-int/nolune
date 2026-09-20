// @ts-check
/**
 * Pure helpers for memory receipts and corrections (#84): the copy behind
 * "Why did Nolune remember this?", receipt keying, conflict prompts and the
 * pin / exclude controls. The server's receipt is the only source: reasons
 * and confidence arrive as buckets, never as scores, and a deleted source is
 * stated rather than dropped. Nothing here builds a URL; the api client does,
 * always under the current companion.
 */

import { displayName, mediaKind } from "./library.js";

/** @typedef {"semantic" | "keyword" | "linked_to" | "matched" | "pinned"} RecallReason */
/** @typedef {"high" | "medium" | "low"} RecallConfidence */
/**
 * @typedef {{
 *   path: string;
 *   source: string;
 *   excerpt: string;
 *   reason: RecallReason | string;
 *   linked_from?: string;
 *   confidence: RecallConfidence | string;
 *   retrieved_at: string;
 *   source_status: "present" | "missing";
 * }} RecalledMemory
 */
/** @typedef {{ message_id: string; chat_id: string; memories: RecalledMemory[] }} MemoryReceipt */
/** @typedef {{ id: string; statement: string; corrected_at: string }} CorrectionStatement */
/** @typedef {{ conflict_id: string; path: string; current: CorrectionStatement; proposed: CorrectionStatement }} CorrectionConflict */
/** @typedef {{ pinned?: boolean; exclude_from_proactive?: boolean }} MemoryFlags */

const PRODUCT_NAME = "Nolune";

/** @param {string | null | undefined} companionName */
export function receiptHeading(companionName) {
	return `Why did ${companionName?.trim() || PRODUCT_NAME} remember this?`;
}

/**
 * How retrieval found the memory, as one sentence.
 * @param {RecalledMemory} memory
 */
export function reasonLabel(memory) {
	switch (memory.reason) {
		case "semantic":
			return "Matches the meaning of the conversation";
		case "keyword":
			return "Contains words from the conversation";
		case "linked_to":
			return memory.linked_from ? `Linked from ${displayName(memory.linked_from)}` : "Linked from another recalled memory";
		case "matched":
			return "Matched the conversation";
		case "pinned":
			return "Pinned by you, recalled every turn";
		default:
			return "Recalled";
	}
}

/** @param {string | undefined} confidence */
export function confidenceLabel(confidence) {
	switch (confidence) {
		case "high":
			return "High confidence";
		case "medium":
			return "Medium confidence";
		case "low":
			return "Low confidence";
		default:
			return "Confidence not reported";
	}
}

/** @param {string | undefined} confidence */
export function confidenceHint(confidence) {
	switch (confidence) {
		case "high":
			return "A strong match.";
		case "medium":
			return "A partial match.";
		case "low":
			return "A weak match; it may not be relevant.";
		default:
			return "";
	}
}

/**
 * Copy for a source that no longer exists; empty while it is present.
 * @param {RecalledMemory} memory
 */
export function sourceStatusCopy(memory) {
	return memory.source_status === "missing"
		? "This memory was forgotten after this reply. The excerpt is what was recalled at the time."
		: "";
}

/**
 * A media memory cites its bound text representation as the source.
 * @param {RecalledMemory} memory
 */
export function isMediaMemory(memory) {
	return memory.source !== memory.path;
}

/** @param {RecalledMemory} memory */
export function sourceLabel(memory) {
	return isMediaMemory(memory) ? `${memory.path} · described in ${memory.source}` : memory.path;
}

/** @param {RecalledMemory[]} memories */
export function receiptSummary(memories) {
	if (memories.length === 0) return "No memories were used for this reply.";
	return memories.length === 1 ? "1 memory shaped this reply" : `${memories.length} memories shaped this reply`;
}

/**
 * Relative time for a recent retrieval, the calendar date otherwise.
 * @param {string} iso RFC 3339 timestamp from the receipt
 * @param {number} [now] milliseconds since the epoch
 */
export function recalledWhen(iso, now = Date.now()) {
	const at = Date.parse(iso);
	if (Number.isNaN(at)) return "";
	const seconds = Math.max(0, Math.floor((now - at) / 1000));
	if (seconds < 60) return "just now";
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
	if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
	if (seconds < 7 * 86400) return `${Math.floor(seconds / 86400)}d ago`;
	return new Date(at).toISOString().slice(0, 10);
}

/**
 * Receipts keyed by message id. Only receipts of `chatId` are kept, so a
 * bubble can never show provenance from another conversation.
 * @template M
 * @param {{ message_id: string; chat_id: string; memories: M[] }[]} receipts
 * @param {string} chatId
 * @returns {Map<string, M[]>}
 */
export function receiptsByMessage(receipts, chatId) {
	/** @type {Map<string, M[]>} */
	const map = new Map();
	if (!chatId) return map;
	for (const receipt of receipts) {
		if (receipt.chat_id !== chatId) continue;
		map.set(receipt.message_id, receipt.memories ?? []);
	}
	return map;
}

/**
 * Every recall of one memory (by library path or canonical source), newest
 * first, with the conversation it happened in.
 * @template {{ path: string; source: string; retrieved_at: string }} M
 * @param {{ message_id: string; chat_id: string; memories: M[] }[]} receipts
 * @param {string} path
 * @returns {{ chat_id: string; message_id: string; memory: M }[]}
 */
export function recallsOf(receipts, path) {
	/** @type {{ chat_id: string; message_id: string; memory: M }[]} */
	const recalls = [];
	for (const receipt of receipts) {
		for (const memory of receipt.memories ?? []) {
			if (memory.path === path || memory.source === path) {
				recalls.push({ chat_id: receipt.chat_id, message_id: receipt.message_id, memory });
			}
		}
	}
	recalls.sort((a, b) => (Date.parse(b.memory.retrieved_at) || 0) - (Date.parse(a.memory.retrieved_at) || 0));
	return recalls;
}

/**
 * The question shown when two corrections of one memory disagree: both
 * statements, in full, and which one each option keeps.
 * @param {CorrectionConflict} conflict
 */
export function conflictPrompt(conflict) {
	return {
		conflictId: conflict.conflict_id,
		path: conflict.path,
		question: `Two of your corrections to ${displayName(conflict.path)} disagree. Which one should stay?`,
		options: [
			{
				keep: /** @type {const} */ ("current"),
				title: "Keep the current statement",
				statement: conflict.current.statement,
				corrected_at: conflict.current.corrected_at,
			},
			{
				keep: /** @type {const} */ ("proposed"),
				title: "Use the new statement",
				statement: conflict.proposed.statement,
				corrected_at: conflict.proposed.corrected_at,
			},
		],
	};
}

/**
 * Classify the body of a PUT memory response.
 * @param {unknown} body
 * @returns {{ kind: "applied" } | { kind: "unchanged" } | { kind: "needs_resolution"; conflict: CorrectionConflict } | { kind: "error"; message: string }}
 */
export function correctionOutcome(body) {
	if (!body || typeof body !== "object") return { kind: "error", message: "The correction was not confirmed." };
	const response = /** @type {Record<string, unknown>} */ (body);
	switch (response.status) {
		case "applied":
			return { kind: "applied" };
		case "unchanged":
			return { kind: "unchanged" };
		case "needs_resolution": {
			const { conflict_id, path, current, proposed } = response;
			if (typeof conflict_id === "string" && typeof path === "string" && isStatement(current) && isStatement(proposed)) {
				return { kind: "needs_resolution", conflict: { conflict_id, path, current, proposed } };
			}
			return { kind: "error", message: "The correction conflicts with an earlier one, but the conflict could not be read." };
		}
		default:
			return { kind: "error", message: typeof response.message === "string" ? response.message : "The correction was not confirmed." };
	}
}

/**
 * @param {unknown} value
 * @returns {value is CorrectionStatement}
 */
function isStatement(value) {
	if (!value || typeof value !== "object") return false;
	const statement = /** @type {Record<string, unknown>} */ (value);
	return typeof statement.id === "string" && typeof statement.statement === "string" && typeof statement.corrected_at === "string";
}

/**
 * The body of a memory file without its stamped `created` / `updated` /
 * flag frontmatter, mirroring the server's `parse_frontmatter`: this is what
 * a correction replaces, so the editor starts from it.
 * @param {string} raw
 */
export function memoryBody(raw) {
	const trimmed = raw.trimStart();
	if (!trimmed.startsWith("---")) return raw.trim();
	const end = trimmed.indexOf("\n---", 3);
	if (end === -1) return raw.trim();
	return trimmed.slice(end + 4).trim();
}

/**
 * Flags live in a text memory's frontmatter; a media memory has none.
 * @param {string} path
 */
export function canFlag(path) {
	return mediaKind(path) === "text";
}

/**
 * The two flag toggles with the label for their next state.
 * @param {MemoryFlags | null | undefined} flags
 */
export function flagControls(flags) {
	const pinned = flags?.pinned === true;
	const excluded = flags?.exclude_from_proactive === true;
	return [
		{
			flag: /** @type {const} */ ("pinned"),
			active: pinned,
			label: pinned ? "Unpin" : "Pin",
			hint: pinned ? "Recalled on every turn." : "Recall this on every turn, whatever the conversation is about.",
			next: { pinned: !pinned },
		},
		{
			flag: /** @type {const} */ ("exclude_from_proactive"),
			active: excluded,
			label: excluded ? "Allow proactive use" : "Exclude from proactive use",
			hint: excluded ? "Check-ins and reflections never see this memory." : "Keep this out of check-ins and reflections; chat still sees it.",
			next: { exclude_from_proactive: !excluded },
		},
	];
}

/** @param {MemoryFlags | null | undefined} flags */
export function flagBadges(flags) {
	const badges = [];
	if (flags?.pinned) badges.push("Pinned");
	if (flags?.exclude_from_proactive) badges.push("Not used proactively");
	return badges;
}
