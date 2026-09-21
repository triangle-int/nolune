// @ts-check
/**
 * The federation inbox (#110): what paired companions asked this owner's
 * companion for, as `GET /api/federation/inbox` lists it, viewed for the
 * Companions section. The two labels a peer declared about itself are its
 * own words and are returned as such; nothing here ever sees a text,
 * because the server keeps none.
 */

import { shortId } from "./companions.js";
import { intentLabel } from "./policy.js";

/**
 * @typedef {import("../api/types.js").FederationInboundIntent} FederationInboundIntent
 * @typedef {import("../api/types.js").FederationIntentReceipt} FederationIntentReceipt
 * @typedef {{
 *   key: string;
 *   peerId: string;
 *   peerShortId: string;
 *   label: string;
 *   representedOwner: string;
 *   purpose: string;
 *   status: "pending" | "accepted" | "denied";
 *   statusLabel: string;
 *   needsOwner: boolean;
 *   approvalId: string | null;
 *   when: string;
 *   note: string;
 * }} InboxRow
 */

/** @type {Record<FederationInboundIntent["status"], string>} */
const STATUS_LABELS = { pending: "Needs you", accepted: "Delivered", denied: "Denied" };

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
 * What the owner can do or what happened, in one sentence.
 *
 * @param {FederationInboundIntent} record
 */
function noteFor(record) {
	switch (record.status) {
		case "pending":
			return record.response.outcome === "needs_owner" && record.response.reason === "quiet_hours"
				? "Held during your quiet hours; it asks again later."
				: "Allow or deny it above; it asks again once you have.";
		case "accepted":
			return record.intent === "reminder" ? "Delivered to your conversation and set as a commitment." : "Delivered to your conversation.";
		case "denied":
			return "Refused by your policy; nothing reached you.";
		default:
			return "";
	}
}

/**
 * One row per live record: requests that need the owner first, then
 * newest first.
 *
 * @param {FederationInboundIntent[]} intents
 * @param {number} now
 * @returns {InboxRow[]}
 */
export function inboxView(intents, now) {
	return intents
		.filter((record) => record.expires_at > now)
		.map((record) => ({
			key: `${record.sender}/${record.correlation_id}`,
			peerId: record.sender,
			peerShortId: shortId(record.sender),
			label: intentLabel(record.intent, record.disclosure),
			representedOwner: record.represented_owner,
			purpose: record.purpose,
			status: record.status,
			statusLabel: STATUS_LABELS[record.status] ?? record.status,
			needsOwner: record.status === "pending",
			approvalId: record.status === "pending" ? (record.approval_id ?? null) : null,
			when: agoLabel(record.updated_at, now),
			note: noteFor(record),
			updatedAt: record.updated_at,
		}))
		.sort((a, b) => Number(b.needsOwner) - Number(a.needsOwner) || b.updatedAt - a.updatedAt)
		.map(({ updatedAt, ...row }) => {
			void updatedAt;
			return row;
		});
}

/**
 * How many records still wait for the owner.
 *
 * @param {FederationInboundIntent[]} intents
 * @returns {number}
 */
export function needsOwnerCount(intents) {
	return intents.filter((record) => record.status === "pending").length;
}
