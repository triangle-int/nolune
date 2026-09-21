// @ts-check
/**
 * Federation policy (#109, PR 3): the pure helpers behind the per-peer
 * capability rows and the pending approvals of the Companions section.
 * Everything is derived from what `GET /api/federation/policy` and
 * `GET /api/federation/approvals` carry and a clock; nothing here ever sees
 * what a peer sent, because the server keeps no such thing either.
 */

function unimplemented() {
	throw new Error("PR 3: not implemented yet");
}

export function intentLabel(intent, disclosure) {
	void intent;
	void disclosure;
	return unimplemented();
}

export function accessLabel(access) {
	void access;
	return unimplemented();
}

export function untilLabel(expiresAt, now) {
	void expiresAt;
	void now;
	return unimplemented();
}

export function capabilityRows(defaults, policy, now) {
	void defaults;
	void policy;
	void now;
	return unimplemented();
}

export function approvalView(approval, now) {
	void approval;
	void now;
	return unimplemented();
}

export function pendingApprovals(approvals, now) {
	void approvals;
	void now;
	return unimplemented();
}

export function decidedApprovals(approvals, now) {
	void approvals;
	void now;
	return unimplemented();
}

export function pendingCountFor(peerId, approvals, now) {
	void peerId;
	void approvals;
	void now;
	return unimplemented();
}

export function approvalScopes() {
	return unimplemented();
}

export function scopeBody(value, now) {
	void value;
	void now;
	return unimplemented();
}

export function decisionErrorText(error) {
	void error;
	return unimplemented();
}
