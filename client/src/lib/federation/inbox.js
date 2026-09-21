// @ts-check
/**
 * The federation inbox (#110): what paired companions asked this owner's
 * companion for, as `GET /api/federation/inbox` lists it, viewed for the
 * Companions section. The two labels a peer declared about itself are its
 * own words and are returned as such; nothing here ever sees a text,
 * because the server keeps none.
 */

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

/**
 * One row per record, newest first, requests that need the owner first.
 *
 * @param {FederationInboundIntent[]} intents
 * @param {number} now
 * @returns {InboxRow[]}
 */
export function inboxView(intents, now) {
	void intents;
	void now;
	return [];
}

/**
 * How many records still wait for the owner.
 *
 * @param {FederationInboundIntent[]} intents
 * @returns {number}
 */
export function needsOwnerCount(intents) {
	void intents;
	return 0;
}
