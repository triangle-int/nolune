// @ts-check
/**
 * Companions (#108, PR 4): the pure helpers behind the Companions section of
 * Settings › Connections. A companion is a peer companion this server's owner
 * paired with, viewed from the record `GET /api/federation/peers` carries;
 * nothing is inferred beyond that record and a clock, and nothing here ever
 * sees an invite secret except the one line the server hands over once.
 */

function pending() {
	throw new Error('#108 PR 4: not implemented yet');
}

export const shortId = pending;
export const lastSeenLabel = pending;
export const peerView = pending;
export const sortPeers = pending;
export const inviteView = pending;
export const inviteHandoff = pending;
export const overviewView = pending;
export const looksLikeUrl = pending;
export const acceptErrorText = pending;
export const rotationSummary = pending;
