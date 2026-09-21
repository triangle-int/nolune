// @ts-check
/**
 * Companions (#108, PR 4): the pure helpers behind the Companions section of
 * Settings › Connections. A companion here is a peer companion this server's
 * owner paired with, viewed from the record `GET /api/federation/peers`
 * carries; nothing is inferred beyond that record and a clock, and nothing
 * here ever sees an invite secret except the one line the server hands over
 * once, which `inviteHandoff` keeps and nothing else copies.
 */

/**
 * @typedef {import("../api/types.js").FederationPeer} FederationPeer
 * @typedef {import("../api/types.js").FederationInvite} FederationInvite
 * @typedef {import("../api/types.js").FederationOverview} FederationOverview
 * @typedef {import("../api/types.js").IssuedFederationInvite} IssuedFederationInvite
 * @typedef {import("../api/types.js").FederationRotationReport} FederationRotationReport
 * @typedef {import("../api/types.js").FederationPeerState} FederationPeerState
 */

/**
 * @typedef {"online" | "warn" | "muted"} PeerTone
 * @typedef {{
 *   id: string;
 *   shortId: string;
 *   publicKey: string;
 *   state: FederationPeerState;
 *   stateLabel: string;
 *   tone: PeerTone;
 *   roleLabel: string;
 *   approvedOrigins: string[];
 *   pendingOrigin: string | null;
 *   lastSeen: string;
 *   lastSeenAt: number | null;
 *   rotations: number;
 *   previousIds: string[];
 *   note: string;
 *   canConfirm: boolean;
 *   canRevoke: boolean;
 * }} PeerView
 * @typedef {{ id: string; countdown: string; expired: boolean; text: string }} InviteView
 * @typedef {{ id: string; line: string; expiresAt: number; origin: string }} InviteHandoff
 * @typedef {{
 *   companionId: string;
 *   publicKey: string;
 *   rotations: number;
 *   previousIds: string[];
 *   invites: InviteView[];
 *   peers: PeerView[];
 *   paired: number;
 *   waitingForYou: number;
 * }} OverviewView
 */

/** Order in the list: trusted first, then what needs attention, then history. */
const STATE_RANK = { paired: 0, pending: 1, invited: 2, revoked: 3 };

/**
 * A companion id is 43 base64url characters; rows show the ends.
 *
 * @param {string} id
 */
export function shortId(id) {
	return id.length <= 14 ? id : `${id.slice(0, 8)}…${id.slice(-4)}`;
}

/**
 * "last seen 2 min ago"; "never seen" when nothing signed by the peer ever
 * verified here. A peer clock ahead of ours reads as just now.
 *
 * @param {number | null | undefined} at unix seconds
 * @param {number} now unix seconds
 */
export function lastSeenLabel(at, now) {
	if (at === null || at === undefined) return "never seen";
	const delta = Math.max(0, now - at);
	if (delta < 90) return "last seen just now";
	if (delta < 3600) return `last seen ${Math.round(delta / 60)} min ago`;
	if (delta < 86400 * 2) return `last seen ${Math.round(delta / 3600)} h ago`;
	return `last seen ${Math.round(delta / 86400)} days ago`;
}

/**
 * The ids a peer had before each rotation this server accepted, oldest first.
 *
 * @param {FederationPeer} peer
 */
function previousIds(peer) {
	return (peer.rotation_history ?? []).map((transition) => transition.rotation.previous.companion_id);
}

/**
 * One row of the Companions list, derived from the record and a clock.
 *
 * @param {FederationPeer} peer
 * @param {number} now unix seconds
 * @returns {PeerView}
 */
export function peerView(peer, now) {
	const issuer = peer.role === "issuer";
	const ids = previousIds(peer);
	const notes = [];
	/** @type {string} */
	let stateLabel;
	/** @type {PeerTone} */
	let tone;
	let canConfirm = false;
	let canRevoke = false;
	switch (peer.state) {
		case "paired":
			stateLabel = "Paired";
			tone = "online";
			canRevoke = true;
			break;
		case "pending":
			tone = "warn";
			canRevoke = true;
			if (issuer) {
				stateLabel = "Waiting for you";
				canConfirm = true;
				notes.push(
					peer.pending_origin
						? `Confirm to pair it; it will be reached at ${peer.pending_origin}.`
						: "Confirm to pair it.",
				);
			} else {
				stateLabel = "Waiting for its owner";
				notes.push("You accepted its invite; its owner still has to confirm on their server.");
			}
			break;
		case "revoked":
			stateLabel = "Revoked";
			tone = "muted";
			notes.push("Trust was withdrawn; a new invite pairs it again.");
			break;
		default:
			stateLabel = "Invited";
			tone = "warn";
			break;
	}
	if (ids.length > 0) {
		const times = ids.length === 1 ? "once" : `${ids.length} times`;
		notes.push(`It rotated its key ${times}; it was ${ids.map(shortId).join(", then ")}.`);
	}
	return {
		id: peer.companion_id,
		shortId: shortId(peer.companion_id),
		publicKey: peer.public_key,
		state: peer.state,
		stateLabel,
		tone,
		roleLabel: issuer ? "You invited it" : "It invited you",
		approvedOrigins: [...(peer.approved_origins ?? [])],
		pendingOrigin: peer.pending_origin ?? null,
		lastSeen: lastSeenLabel(peer.last_seen_at, now),
		lastSeenAt: peer.last_seen_at ?? null,
		rotations: ids.length,
		previousIds: ids,
		note: notes.join(" "),
		canConfirm,
		canRevoke,
	};
}

/**
 * Paired first, then pending, then revoked; within a state the most
 * recently seen first, never-seen last.
 *
 * @param {FederationPeer[]} peers
 */
export function sortPeers(peers) {
	return [...peers].sort((a, b) => {
		const rank = (STATE_RANK[a.state] ?? 9) - (STATE_RANK[b.state] ?? 9);
		if (rank !== 0) return rank;
		return (b.last_seen_at ?? -1) - (a.last_seen_at ?? -1);
	});
}

/**
 * "m:ss" until `expiresAt`, floored at zero.
 *
 * @param {number} expiresAt unix seconds
 * @param {number} now unix seconds
 */
export function inviteCountdown(expiresAt, now) {
	const left = Math.max(0, expiresAt - now);
	const m = Math.floor(left / 60);
	const s = left % 60;
	return `${m}:${String(s).padStart(2, "0")}`;
}

/**
 * An outstanding invite as the list shows it: a handle and a clock, never a
 * secret (the server does not send one here either).
 *
 * @param {FederationInvite} invite
 * @param {number} now unix seconds
 * @returns {InviteView}
 */
export function inviteView(invite, now) {
	const expired = invite.expires_at <= now;
	const countdown = inviteCountdown(invite.expires_at, now);
	return {
		id: invite.id,
		countdown,
		expired,
		text: expired ? `Invite ${invite.id} expired` : `Invite ${invite.id} expires in ${countdown}`,
	};
}

/**
 * What the one-time panel keeps from a freshly minted invite: the line to
 * hand over, when it expires, and where the peer will reach this server.
 * The raw secret field is not copied; it lives in the line and nowhere else.
 *
 * @param {IssuedFederationInvite} issued
 * @returns {InviteHandoff}
 */
export function inviteHandoff(issued) {
	return { id: issued.id, line: issued.invite, expiresAt: issued.expires_at, origin: issued.origin };
}

/**
 * The whole listing viewed for the section.
 *
 * @param {FederationOverview} overview
 * @param {number} now unix seconds
 * @returns {OverviewView}
 */
export function overviewView(overview, now) {
	const peers = sortPeers(overview.peers ?? []).map((peer) => peerView(peer, now));
	return {
		companionId: overview.companion_id,
		publicKey: overview.identity?.public_key ?? "",
		rotations: (overview.rotations ?? []).length,
		previousIds: (overview.rotations ?? []).map((rotation) => rotation.previous.companion_id),
		invites: (overview.invites ?? []).map((invite) => inviteView(invite, now)),
		peers,
		paired: peers.filter((peer) => peer.state === "paired").length,
		waitingForYou: peers.filter((peer) => peer.canConfirm).length,
	};
}

/**
 * Whether pasted text is shaped like a URL. An invite is never one, and one
 * is never fetched from one: such input is refused before it is sent.
 *
 * @param {string | null | undefined} text
 */
export function looksLikeUrl(text) {
	const trimmed = (text ?? "").trim();
	const lowered = trimmed.toLowerCase();
	return lowered.startsWith("http://") || lowered.startsWith("https://") || trimmed.includes("://");
}

/**
 * A sentence for the accepting owner per server refusal. The server's own
 * message is never echoed: it could describe the line that was pasted.
 *
 * @param {unknown} error a `FederationApiError`-shaped object, an `Error`, or nothing
 */
export function acceptErrorText(error) {
	const code = typeof error === "object" && error !== null && "code" in error ? String(error.code) : "";
	const peerError =
		typeof error === "object" && error !== null && "peerError" in error && error.peerError ? String(error.peerError) : "";
	switch (code) {
		case "invalid_invite":
			return "That invite has already been used, expired, or is wrong. Ask the other owner for a new one.";
		case "malformed":
		case "invalid_body":
			return "That is not an invite line. Paste the whole nolune-invite line the other owner gave you.";
		case "invalid_origin":
			return "The invite names an address this server cannot use. Ask the other owner to set their public address and invite again.";
		case "peer_unreachable":
			return "The other companion could not be reached at the address in the invite. Check that its server is running and reachable from here.";
		case "peer_refused":
			if (peerError === "invalid_invite") {
				return "The other companion refused: the invite has already been used, expired, or is wrong. Ask for a new one.";
			}
			return `The other companion refused the invite${peerError ? ` (${peerError.replaceAll("_", " ")})` : ""}.`;
		case "issuer_mismatch":
			return "That invite was minted by this companion itself; it cannot pair with itself.";
		case "federation_unavailable":
			return "This server's federation files are unusable; check the server log before pairing.";
		default:
			return "Could not accept the invite. Check that this server is running and try again.";
	}
}

/**
 * One sentence about a finished rotation: the new id, who was told, and who
 * still needs the proof.
 *
 * @param {FederationRotationReport} report
 */
export function rotationSummary(report) {
	const parts = [`Your companion is now ${report.identity.companion_id}.`];
	const told = report.notified?.length ?? 0;
	if (told === 0) {
		parts.push("No companions were told.");
	} else {
		parts.push(`${told} ${told === 1 ? "companion was" : "companions were"} told.`);
	}
	if (report.unreachable?.length) {
		parts.push(
			`${report.unreachable.map(shortId).join(", ")} could not be reached and ${report.unreachable.length === 1 ? "keeps" : "keep"} trusting the old key until ${report.unreachable.length === 1 ? "it hears" : "they hear"} the proof.`,
		);
	}
	return parts.join(" ");
}
