import test from 'node:test';
import assert from 'node:assert/strict';
import {
	accessLabel,
	approvalScopes,
	approvalView,
	capabilityRows,
	decidedApprovals,
	decisionErrorText,
	intentLabel,
	pendingApprovals,
	pendingCountFor,
	scopeBody,
	untilLabel,
} from '../src/lib/federation/policy.js';

const NOW = 1_800_000_000;
const PEER = 'TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E';
const OTHER = 'SN7Fvp7FYlYfvTUUW4vPFzy1jh9gtfDVlA7I3_QSj4U';

/** The defaults table as `GET /api/federation/policy` lists it. */
const DEFAULTS = [
	{ intent: 'ping', disclosure: 'none', access: 'allow' },
	{ intent: 'message', disclosure: 'none', access: 'ask' },
	{ intent: 'availability', disclosure: 'availability', access: 'ask' },
	{ intent: 'availability', disclosure: 'personal', access: 'deny' },
	{ intent: 'availability', disclosure: 'sensitive', access: 'deny' },
	{ intent: 'reminder', disclosure: 'none', access: 'ask' },
	{ intent: 'proposal', disclosure: 'none', access: 'ask' },
	{ intent: 'proposal', disclosure: 'availability', access: 'ask' },
	{ intent: 'proposal', disclosure: 'personal', access: 'deny' },
	{ intent: 'proposal', disclosure: 'sensitive', access: 'deny' },
	{ intent: 'handoff', disclosure: 'none', access: 'ask' },
	{ intent: 'decision', disclosure: 'none', access: 'allow' },
];

function rule(overrides = {}) {
	return { intent: 'message', disclosure: 'none', access: 'allow', granted_at: NOW - 600, expires_at: undefined, ...overrides };
}

function approval(overrides = {}) {
	return {
		version: 1,
		id: '0123456789abcdef',
		pairing_id: '9f1c0b7e2a6d4c31',
		requester: PEER,
		intent: 'message',
		disclosure: 'none',
		status: 'pending',
		requested_at: NOW - 120,
		expires_at: NOW - 120 + 86_400,
		summary: 'companion asks for message (none)',
		...overrides,
	};
}

test('every supported intent and class reads as a sentence about the owner', () => {
	assert.equal(intentLabel('ping', 'none'), 'check that it can reach you');
	assert.equal(intentLabel('message', 'none'), 'send you a message');
	assert.equal(intentLabel('reminder', 'none'), 'leave you a reminder');
	assert.equal(intentLabel('availability', 'availability'), 'ask whether you are free');
	assert.equal(intentLabel('availability', 'personal'), 'ask whether you are free and what you are doing');
	assert.equal(intentLabel('availability', 'sensitive'), 'ask whether you are free, including what you marked sensitive');
	assert.equal(intentLabel('proposal', 'none'), 'propose something to you');
	assert.equal(intentLabel('proposal', 'availability'), 'propose something and see whether you are free');
	assert.equal(intentLabel('proposal', 'personal'), 'propose something and see what you are doing');
	assert.equal(intentLabel('proposal', 'sensitive'), 'propose something and see what you marked sensitive');
	assert.equal(intentLabel('handoff', 'none'), 'hand an unfinished task over to you');
	assert.equal(intentLabel('decision', 'none'), 'tell you what its owner decided on what you proposed');
	for (const row of DEFAULTS) {
		assert.notEqual(intentLabel(row.intent, row.disclosure), `${row.intent} (${row.disclosure})`, `${row.intent}/${row.disclosure} has no sentence`);
	}
	// Anything the server might add later still reads, without inventing.
	assert.equal(intentLabel('handshake', 'none'), 'handshake (none)');
	assert.equal(accessLabel('allow'), 'Allowed');
	assert.equal(accessLabel('ask'), 'Asks you');
	assert.equal(accessLabel('deny'), 'Denied');
	assert.equal(accessLabel('maybe'), 'maybe');
});

test('a deadline reads relative to now and no deadline reads as until revoked', () => {
	assert.equal(untilLabel(null, NOW), 'until revoked');
	assert.equal(untilLabel(undefined, NOW), 'until revoked');
	assert.equal(untilLabel(NOW + 30, NOW), 'for 1 min more');
	assert.equal(untilLabel(NOW + 45 * 60, NOW), 'for 45 min more');
	assert.equal(untilLabel(NOW + 3 * 3600, NOW), 'for 3 h more');
	assert.equal(untilLabel(NOW + 5 * 86_400, NOW), 'for 5 days more');
	assert.equal(untilLabel(NOW, NOW), 'lapsed');
	assert.equal(untilLabel(NOW - 5, NOW), 'lapsed');
});

test('capability rows follow the defaults table and say what applies now', () => {
	const rows = capabilityRows(DEFAULTS, { rules: [] }, NOW);
	assert.equal(rows.length, DEFAULTS.length);
	assert.deepEqual(
		rows.map((row) => row.key),
		DEFAULTS.map((row) => `${row.intent}/${row.disclosure}`),
		'rows keep the server order',
	);
	const message = rows.find((row) => row.key === 'message/none');
	assert.equal(message.intent, 'message');
	assert.equal(message.disclosure, 'none');
	assert.equal(message.label, 'send you a message');
	assert.equal(message.defaultAccess, 'ask');
	assert.equal(message.effective, 'ask');
	assert.equal(message.effectiveLabel, 'Asks you');
	assert.equal(message.source, 'default');
	assert.equal(message.expiresAt, null);
	assert.equal(message.canRevoke, false);
	assert.equal(message.note, '');
	const ping = rows.find((row) => row.key === 'ping/none');
	assert.equal(ping.effective, 'allow');
	assert.equal(ping.source, 'default');
	const sensitive = rows.find((row) => row.key === 'proposal/sensitive');
	assert.equal(sensitive.effective, 'deny');
	// A missing policy is the empty one.
	assert.equal(capabilityRows(DEFAULTS, undefined, NOW).length, DEFAULTS.length);
});

test('a live rule replaces the default, an expired one is reported, the strictest duplicate wins', () => {
	const policy = {
		rules: [
			rule({ expires_at: NOW + 3600 }),
			rule({ intent: 'availability', disclosure: 'availability', access: 'deny' }),
			rule({ intent: 'reminder', disclosure: 'none', access: 'allow', expires_at: NOW - 1 }),
			rule({ intent: 'proposal', disclosure: 'none', access: 'allow' }),
			rule({ intent: 'proposal', disclosure: 'none', access: 'ask' }),
			rule({ intent: 'ping', disclosure: 'personal', access: 'allow' }),
		],
	};
	const rows = capabilityRows(DEFAULTS, policy, NOW);
	assert.equal(rows.length, DEFAULTS.length, 'a rule for a pair the intent cannot disclose at is not a row');
	const message = rows.find((row) => row.key === 'message/none');
	assert.equal(message.effective, 'allow');
	assert.equal(message.source, 'rule');
	assert.equal(message.expiresAt, NOW + 3600);
	assert.equal(message.expiresLabel, 'for 1 h more');
	assert.equal(message.canRevoke, true);
	assert.match(message.note, /^Set by you/);
	const availability = rows.find((row) => row.key === 'availability/availability');
	assert.equal(availability.effective, 'deny');
	assert.equal(availability.source, 'rule');
	assert.equal(availability.expiresLabel, 'until revoked');
	const reminder = rows.find((row) => row.key === 'reminder/none');
	assert.equal(reminder.effective, 'ask', 'the default applies after a rule lapsed');
	assert.equal(reminder.source, 'expired');
	assert.equal(reminder.canRevoke, false);
	assert.match(reminder.note, /lapsed/);
	const proposal = rows.find((row) => row.key === 'proposal/none');
	assert.equal(proposal.effective, 'ask', 'the most restrictive duplicate wins, as on the server');
	assert.equal(proposal.source, 'rule');
});

test('a pending request reads as what the companion wants and how long it waits', () => {
	const view = approvalView(approval(), NOW);
	assert.equal(view.id, '0123456789abcdef');
	assert.equal(view.peerId, PEER);
	assert.match(view.peerShortId, /^TFccHElq…cQ7E$/);
	assert.equal(view.label, 'send you a message');
	assert.equal(view.status, 'pending');
	assert.equal(view.statusLabel, 'Waiting for you');
	assert.equal(view.isPending, true);
	assert.equal(view.canWithdraw, true);
	assert.equal(view.asked, 'asked 2 min ago');
	assert.equal(view.expires, 'lapses in 24 h');
	const approved = approvalView(approval({ status: 'approved', decided_at: NOW - 60, expires_at: NOW + 3540 }), NOW);
	assert.equal(approved.statusLabel, 'Allowed once');
	assert.equal(approved.isPending, false);
	assert.equal(approved.canWithdraw, true);
	assert.equal(approved.expires, 'lapses in 59 min');
	const denied = approvalView(approval({ status: 'denied', decided_at: NOW - 60 }), NOW);
	assert.equal(denied.statusLabel, 'Denied');
	assert.equal(denied.isPending, false);
	assert.equal(denied.canWithdraw, true);
	const lapsed = approvalView(approval({ expires_at: NOW }), NOW);
	assert.equal(lapsed.expires, 'lapsed');
	// The view carries names and a clock, never a text.
	assert.equal('text' in view, false);
	assert.equal('body' in view, false);
});

test('pending requests are listed newest first and decided ones apart, lapsed ones not at all', () => {
	const all = [
		approval({ id: 'a', requested_at: NOW - 300 }),
		approval({ id: 'b', requested_at: NOW - 30, intent: 'reminder' }),
		approval({ id: 'c', status: 'approved', decided_at: NOW - 10, expires_at: NOW + 3590 }),
		approval({ id: 'd', status: 'denied', decided_at: NOW - 10 }),
		approval({ id: 'e', requested_at: NOW - 90_000, expires_at: NOW - 3600 }),
		approval({ id: 'f', requester: OTHER, requested_at: NOW - 10, intent: 'proposal' }),
	];
	assert.deepEqual(
		pendingApprovals(all, NOW).map((view) => view.id),
		['f', 'b', 'a'],
	);
	assert.deepEqual(
		decidedApprovals(all, NOW).map((view) => view.id),
		['c', 'd'],
	);
	assert.equal(pendingCountFor(PEER, all, NOW), 2);
	assert.equal(pendingCountFor(OTHER, all, NOW), 1);
	assert.equal(pendingCountFor('nobody', all, NOW), 0);
	assert.deepEqual(pendingApprovals(undefined, NOW), []);
});

test('the scopes offered are bounded and each one becomes the body the server takes', () => {
	const scopes = approvalScopes();
	assert.deepEqual(
		scopes.map((scope) => scope.value),
		['once', 'day', 'week', 'class'],
	);
	assert.equal(scopes[0].label, 'Once');
	assert.equal(scopes[1].label, 'For a day');
	assert.equal(scopes[2].label, 'For a week');
	assert.equal(scopes[3].label, 'Always for this kind of request');
	assert.deepEqual(scopeBody('once', NOW), { scope: 'once' });
	assert.deepEqual(scopeBody('day', NOW), { scope: 'until', expires_at: NOW + 86_400 });
	assert.deepEqual(scopeBody('week', NOW), { scope: 'until', expires_at: NOW + 7 * 86_400 });
	assert.deepEqual(scopeBody('class', NOW), { scope: 'class' });
	assert.deepEqual(scopeBody('forever', NOW), { scope: 'once' }, 'anything else is the narrowest scope');
});

test('every refusal from the owner routes reads as one sentence without echoing the server', () => {
	assert.match(decisionErrorText({ code: 'unknown_approval' }), /already decided, withdrawn, or lapsed/);
	assert.match(decisionErrorText({ code: 'unknown_rule' }), /no rule/);
	assert.match(decisionErrorText({ code: 'unknown_peer' }), /not paired/);
	assert.match(decisionErrorText({ code: 'peer_revoked' }), /revoked/);
	assert.match(decisionErrorText({ code: 'malformed' }), /cannot/);
	assert.match(decisionErrorText({ code: 'federation_unavailable' }), /server log/);
	assert.match(decisionErrorText(new Error('ECONNREFUSED at http://secret.internal')), /try again/);
	assert.doesNotMatch(decisionErrorText(new Error('ECONNREFUSED at http://secret.internal')), /secret/);
	assert.match(decisionErrorText(undefined), /try again/);
});
