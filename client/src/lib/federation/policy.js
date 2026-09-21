// @ts-check
/**
 * Federation policy (#109, PR 3): the pure helpers behind the per-peer
 * capability rows and the pending approvals of the Companions section.
 * Everything is derived from what `GET /api/federation/policy` and
 * `GET /api/federation/approvals` carry and a clock; nothing here ever sees
 * what a peer sent, because the server keeps no such thing either.
 */

import { shortId } from "./companions.js";

/**
 * @typedef {import("../api/types.js").FederationApproval} FederationApproval
 * @typedef {import("../api/types.js").FederationApprovalStatus} FederationApprovalStatus
 * @typedef {import("../api/types.js").FederationAccess} FederationAccess
 * @typedef {import("../api/types.js").FederationDefaultAccess} FederationDefaultAccess
 * @typedef {import("../api/types.js").FederationPeerPolicy} FederationPeerPolicy
 * @typedef {import("../api/types.js").FederationPolicyRule} FederationPolicyRule
 * @typedef {import("../api/types.js").FederationApprovalScope} FederationApprovalScope
 */

/**
 * @typedef {"default" | "rule" | "expired"} CapabilitySource
 * @typedef {{
 *   key: string;
 *   intent: string;
 *   disclosure: string;
 *   label: string;
 *   defaultAccess: FederationAccess;
 *   effective: FederationAccess;
 *   effectiveLabel: string;
 *   source: CapabilitySource;
 *   expiresAt: number | null;
 *   expiresLabel: string;
 *   canRevoke: boolean;
 *   note: string;
 * }} CapabilityRow
 * @typedef {{
 *   id: string;
 *   peerId: string;
 *   peerShortId: string;
 *   intent: string;
 *   disclosure: string;
 *   label: string;
 *   status: FederationApprovalStatus;
 *   statusLabel: string;
 *   isPending: boolean;
 *   canWithdraw: boolean;
 *   asked: string;
 *   expires: string;
 * }} ApprovalView
 * @typedef {{ value: "once" | "day" | "week" | "class"; label: string }} ScopeOption
 */

/**
 * What each intent at each disclosure class means for this owner.
 *
 * @type {Record<string, string>}
 */
const INTENT_LABELS = {
	"ping/none": "check that it can reach you",
	"message/none": "send you a message",
	"reminder/none": "leave you a reminder",
	"availability/availability": "ask whether you are free",
	"availability/personal": "ask whether you are free and what you are doing",
	"availability/sensitive": "ask whether you are free, including what you marked sensitive",
	"proposal/none": "propose something to you",
	"proposal/availability": "propose something and see whether you are free",
	"proposal/personal": "propose something and see what you are doing",
	"proposal/sensitive": "propose something and see what you marked sensitive",
};

/** @type {Record<string, string>} */
const ACCESS_LABELS = { allow: "Allowed", ask: "Asks you", deny: "Denied" };

/**
 * The most restrictive of two rules wins, as in the server's engine.
 *
 * @type {Record<FederationAccess, number>}
 */
const RESTRICTION = { allow: 0, ask: 1, deny: 2 };

/**
 * One sentence for what a peer may do: "send you a message". Anything the
 * server names that this build has no sentence for reads as its names.
 *
 * @param {string} intent
 * @param {string} disclosure
 */
export function intentLabel(intent, disclosure) {
	return INTENT_LABELS[`${intent}/${disclosure}`] ?? `${intent} (${disclosure})`;
}

/**
 * @param {string} access
 */
export function accessLabel(access) {
	return ACCESS_LABELS[access] ?? access;
}

/**
 * How long a rule or an entry still holds: "for 3 h more", "until
 * revoked" when it has no deadline, "lapsed" once it passed.
 *
 * @param {number | null | undefined} expiresAt unix seconds
 * @param {number} now unix seconds
 */
export function untilLabel(expiresAt, now) {
	if (expiresAt === null || expiresAt === undefined) return "until revoked";
	const left = expiresAt - now;
	if (left <= 0) return "lapsed";
	if (left < 3600) return `for ${Math.max(1, Math.round(left / 60))} min more`;
	if (left < 86400 * 2) return `for ${Math.round(left / 3600)} h more`;
	return `for ${Math.round(left / 86400)} days more`;
}

/**
 * "lapses in 23 h", or "lapsed".
 *
 * @param {number} expiresAt unix seconds
 * @param {number} now unix seconds
 */
function lapsesLabel(expiresAt, now) {
	const left = expiresAt - now;
	if (left <= 0) return "lapsed";
	if (left < 3600) return `lapses in ${Math.max(1, Math.round(left / 60))} min`;
	if (left < 86400 * 2) return `lapses in ${Math.round(left / 3600)} h`;
	return `lapses in ${Math.round(left / 86400)} days`;
}

/**
 * "asked 2 min ago"; a request stamped ahead of our clock reads as just now.
 *
 * @param {number} at unix seconds
 * @param {number} now unix seconds
 */
function askedLabel(at, now) {
	const delta = Math.max(0, now - at);
	if (delta < 90) return "asked just now";
	if (delta < 3600) return `asked ${Math.round(delta / 60)} min ago`;
	if (delta < 86400 * 2) return `asked ${Math.round(delta / 3600)} h ago`;
	return `asked ${Math.round(delta / 86400)} days ago`;
}

/**
 * One row per pair the server could ever allow, in the server's order:
 * what applies to the peer now (a live rule, or the default, reported as
 * such when a rule lapsed), with the most restrictive duplicate winning as
 * on the server. A rule for a pair that is not in the defaults is not a
 * capability and is not shown.
 *
 * @param {FederationDefaultAccess[]} defaults
 * @param {FederationPeerPolicy | undefined | null} policy
 * @param {number} now unix seconds
 * @returns {CapabilityRow[]}
 */
export function capabilityRows(defaults, policy, now) {
	const rules = policy?.rules ?? [];
	return defaults.map((row) => {
		const matching = rules.filter((rule) => rule.intent === row.intent && rule.disclosure === row.disclosure);
		const live = matching.filter((rule) => rule.expires_at === undefined || rule.expires_at === null || rule.expires_at > now);
		/** @type {FederationPolicyRule | null} */
		let applied = null;
		for (const rule of live) {
			if (!applied || RESTRICTION[rule.access] > RESTRICTION[applied.access]) applied = rule;
		}
		/** @type {CapabilitySource} */
		const source = applied ? "rule" : matching.length > 0 ? "expired" : "default";
		const effective = applied ? applied.access : row.access;
		const expiresAt = applied?.expires_at ?? null;
		let note = "";
		if (source === "rule") note = `Set by you, ${untilLabel(expiresAt, now)}.`;
		if (source === "expired") note = "Your earlier rule lapsed; the default applies again.";
		return {
			key: `${row.intent}/${row.disclosure}`,
			intent: row.intent,
			disclosure: row.disclosure,
			label: intentLabel(row.intent, row.disclosure),
			defaultAccess: row.access,
			effective,
			effectiveLabel: accessLabel(effective),
			source,
			expiresAt,
			expiresLabel: applied ? untilLabel(expiresAt, now) : "",
			canRevoke: source === "rule",
			note,
		};
	});
}

/**
 * One request that asked the owner, as the section shows it: who wants to
 * do what, where it stands, and its clocks. Names only, never a text.
 *
 * @param {FederationApproval} approval
 * @param {number} now unix seconds
 * @returns {ApprovalView}
 */
export function approvalView(approval, now) {
	/** @type {string} */
	let statusLabel;
	switch (approval.status) {
		case "approved":
			statusLabel = "Allowed once";
			break;
		case "denied":
			statusLabel = "Denied";
			break;
		default:
			statusLabel = "Waiting for you";
			break;
	}
	return {
		id: approval.id,
		peerId: approval.requester,
		peerShortId: shortId(approval.requester),
		intent: approval.intent,
		disclosure: approval.disclosure,
		label: intentLabel(approval.intent, approval.disclosure),
		status: approval.status,
		statusLabel,
		isPending: approval.status === "pending",
		canWithdraw: true,
		asked: askedLabel(approval.requested_at, now),
		expires: lapsesLabel(approval.expires_at, now),
	};
}

/**
 * @param {FederationApproval[] | undefined | null} approvals
 * @param {number} now
 */
function live(approvals, now) {
	return (approvals ?? []).filter((approval) => approval.expires_at > now);
}

/**
 * What needs the owner: pending, unlapsed requests, newest first.
 *
 * @param {FederationApproval[] | undefined | null} approvals
 * @param {number} now unix seconds
 * @returns {ApprovalView[]}
 */
export function pendingApprovals(approvals, now) {
	return live(approvals, now)
		.filter((approval) => approval.status === "pending")
		.sort((a, b) => b.requested_at - a.requested_at)
		.map((approval) => approvalView(approval, now));
}

/**
 * What the owner already said and can still take back: approvals once
 * waiting for the companion to come back, and denials once still holding.
 *
 * @param {FederationApproval[] | undefined | null} approvals
 * @param {number} now unix seconds
 * @returns {ApprovalView[]}
 */
export function decidedApprovals(approvals, now) {
	return live(approvals, now)
		.filter((approval) => approval.status !== "pending")
		.sort((a, b) => (b.decided_at ?? b.requested_at) - (a.decided_at ?? a.requested_at))
		.map((approval) => approvalView(approval, now));
}

/**
 * How many requests from `peerId` wait for the owner.
 *
 * @param {string} peerId
 * @param {FederationApproval[] | undefined | null} approvals
 * @param {number} now unix seconds
 */
export function pendingCountFor(peerId, approvals, now) {
	return live(approvals, now).filter((approval) => approval.status === "pending" && approval.requester === peerId).length;
}

/**
 * The bounds an owner can give an answer, narrowest first. "Once" and
 * "always for this kind" are the server's `once` and `class`; the two in
 * between are deadlines counted from the moment of the click.
 *
 * @returns {ScopeOption[]}
 */
export function approvalScopes() {
	return [
		{ value: "once", label: "Once" },
		{ value: "day", label: "For a day" },
		{ value: "week", label: "For a week" },
		{ value: "class", label: "Always for this kind of request" },
	];
}

/**
 * The body `POST /api/federation/approvals/{id}/approve` (or `/deny`)
 * takes for a chosen scope; anything unknown is the narrowest one.
 *
 * @param {string} value
 * @param {number} now unix seconds
 * @returns {FederationApprovalScope}
 */
export function scopeBody(value, now) {
	switch (value) {
		case "day":
			return { scope: "until", expires_at: now + 86400 };
		case "week":
			return { scope: "until", expires_at: now + 7 * 86400 };
		case "class":
			return { scope: "class" };
		default:
			return { scope: "once" };
	}
}

/**
 * A sentence per server refusal from the approval and rule routes. The
 * server's own message is never echoed.
 *
 * @param {unknown} error a `FederationApiError`-shaped object, an `Error`, or nothing
 */
export function decisionErrorText(error) {
	const code = typeof error === "object" && error !== null && "code" in error ? String(error.code) : "";
	switch (code) {
		case "unknown_approval":
			return "That request was already decided, withdrawn, or lapsed.";
		case "unknown_rule":
			return "There is no rule to revoke for that; the default already applies.";
		case "unknown_peer":
			return "That companion is not paired here any more.";
		case "peer_revoked":
			return "That companion was revoked; it keeps no rules until a new invite pairs it again.";
		case "malformed":
			return "That cannot be granted: the deadline has passed or the request cannot disclose at that class.";
		case "invalid_body":
			return "This page sent something the server did not expect; reload and try again.";
		case "federation_unavailable":
			return "This server's federation files are unusable; check the server log before deciding.";
		default:
			return "Could not save the decision. Check that this server is running and try again.";
	}
}
