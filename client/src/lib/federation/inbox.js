// @ts-check
/**
 * The federation inbox (#110): what paired companions asked this owner's
 * companion for, as `GET /api/federation/inbox` lists it, viewed for the
 * Companions section. The two labels a peer declared about itself are its
 * own words and are returned as such; nothing here ever sees a text,
 * because the server keeps none.
 *
 * A request that asked the owner is settled on the server only when the
 * peer asks again (the approval once is what admits or refuses the next
 * delivery), so between the owner's decision and that redelivery the
 * record still reads `pending`. Given the approval queue, a row whose
 * approval the owner already decided says so instead of asking again.
 */

import { shortId } from "./companions.js";
import { intentLabel } from "./policy.js";

/**
 * @typedef {import("../api/types.js").FederationInboundIntent} FederationInboundIntent
 * @typedef {import("../api/types.js").FederationIntentReceipt} FederationIntentReceipt
 * @typedef {import("../api/types.js").FederationApproval} FederationApproval
 * @typedef {"pending" | "approved" | "accepted" | "denied"} InboxTone
 * @typedef {{
 *   key: string;
 *   peerId: string;
 *   peerShortId: string;
 *   label: string;
 *   representedOwner: string;
 *   purpose: string;
 *   status: "pending" | "accepted" | "denied";
 *   tone: InboxTone;
 *   statusLabel: string;
 *   needsOwner: boolean;
 *   approvalId: string | null;
 *   when: string;
 *   note: string;
 * }} InboxRow
 */

/** @type {Record<InboxTone, string>} */
const TONE_LABELS = { pending: "Needs you", approved: "Allowed", accepted: "Delivered", denied: "Denied" };

/**
 * "2 min ago"; a record stamped ahead of our clock reads as just now.
 *
 * @param {number} at unix seconds
 * @param {number} now unix seconds
 */
function agoLabel(at, now) {
	const delta = Math.max(0, now - at);
	if (delta < 90) return "just now";
	if (delta < 3600) return `${Math.round(delta / 60)} min ago`;
	if (delta < 86400 * 2) return `${Math.round(delta / 3600)} h ago`;
	return `${Math.round(delta / 86400)} days ago`;
}

/**
 * The owner's word on a pending record, when they already gave it: the
 * live approval it links, decided and not lapsed. Anything else (still
 * pending, another entry, lapsed, another pairing) is no word at all.
 *
 * @param {FederationInboundIntent} record
 * @param {FederationApproval[]} approvals
 * @param {number} now
 * @returns {"approved" | "denied" | null}
 */
function ownersWord(record, approvals, now) {
	if (record.status !== "pending" || !record.approval_id) return null;
	const entry = approvals.find((approval) => approval.id === record.approval_id && approval.pairing_id === record.pairing_id && approval.expires_at > now);
	if (!entry) return null;
	return entry.status === "approved" || entry.status === "denied" ? entry.status : null;
}

/**
 * Where the row stands for the owner: what the server says, or their own
 * decision the peer has not collected yet.
 *
 * @param {FederationInboundIntent} record
 * @param {"approved" | "denied" | null} word
 * @returns {InboxTone}
 */
function toneFor(record, word) {
	if (record.status === "pending") return word ?? "pending";
	return record.status;
}

/**
 * What the owner can do or what happened, in one sentence.
 *
 * @param {FederationInboundIntent} record
 * @param {"approved" | "denied" | null} word
 */
function noteFor(record, word) {
	switch (record.status) {
		case "pending":
			if (word === "denied") return "You denied it once; it is refused when the companion asks again.";
			if (word === "approved") return "You allowed it once; it is delivered when the companion asks again.";
			return record.response.outcome === "needs_owner" && record.response.reason === "quiet_hours"
				? "Held during your quiet hours; it asks again later."
				: "Allow or deny it above; it asks again once you have.";
		case "accepted":
			return record.intent === "reminder" ? "Delivered to your conversation and set as a commitment." : "Delivered to your conversation.";
		case "denied":
			return record.reason === "owner_denied" ? "You denied this once; nothing reached you." : "Refused by your policy; nothing reached you.";
		default:
			return "";
	}
}

/**
 * One row per live record: requests that need the owner first, then
 * newest first. `approvals` is the queue from `GET /api/federation/approvals`,
 * so a request the owner already decided stops asking them.
 *
 * @param {FederationInboundIntent[]} intents
 * @param {number} now
 * @param {FederationApproval[]} [approvals]
 * @returns {InboxRow[]}
 */
export function inboxView(intents, now, approvals = []) {
	return intents
		.filter((record) => record.expires_at > now)
		.map((record) => {
			const word = ownersWord(record, approvals, now);
			const tone = toneFor(record, word);
			const needsOwner = tone === "pending";
			return {
				key: `${record.sender}/${record.correlation_id}`,
				peerId: record.sender,
				peerShortId: shortId(record.sender),
				label: intentLabel(record.intent, record.disclosure),
				representedOwner: record.represented_owner,
				purpose: record.purpose,
				status: record.status,
				tone,
				statusLabel: TONE_LABELS[tone],
				needsOwner,
				approvalId: needsOwner ? (record.approval_id ?? null) : null,
				when: agoLabel(record.updated_at, now),
				note: noteFor(record, word),
				updatedAt: record.updated_at,
			};
		})
		.sort((a, b) => Number(b.needsOwner) - Number(a.needsOwner) || b.updatedAt - a.updatedAt)
		.map(({ updatedAt, ...row }) => {
			void updatedAt;
			return row;
		});
}

/**
 * How many records still wait for the owner's word.
 *
 * @param {FederationInboundIntent[]} intents
 * @param {number} now
 * @param {FederationApproval[]} [approvals]
 * @returns {number}
 */
export function needsOwnerCount(intents, now, approvals = []) {
	return inboxView(intents, now, approvals).filter((row) => row.needsOwner).length;
}
