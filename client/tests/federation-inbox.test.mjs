import test from 'node:test';
import assert from 'node:assert/strict';
import { inboxView, needsOwnerCount } from '../src/lib/federation/inbox.js';

const NOW = 1_800_000_000;
const PEER = 'TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E';

function record(overrides = {}) {
	return {
		version: 1,
		sender: PEER,
		correlation_id: 'req-1',
		pairing_id: '9f1c0b7e2a6d4c31',
		intent: 'message',
		disclosure: 'none',
		represented_owner: 'Alice',
		purpose: 'catch up after the trip',
		status: 'pending',
		response: { outcome: 'needs_owner', version: 1, correlation_id: 'req-1', responder: 'x', reason: 'default' },
		approval_id: '0123456789abcdef',
		requested_at: NOW - 120,
		updated_at: NOW - 120,
		expires_at: NOW + 3_480,
		...overrides,
	};
}

test('a pending request reads as what the peer wants, on whose behalf, and why, and needs the owner', () => {
	const [row] = inboxView([record()], NOW);
	assert.equal(row.key, `${PEER}/req-1`);
	assert.equal(row.peerId, PEER);
	assert.equal(row.peerShortId, 'TFccHElq…cQ7E');
	assert.equal(row.label, 'send you a message');
	assert.equal(row.representedOwner, 'Alice');
	assert.equal(row.purpose, 'catch up after the trip');
	assert.equal(row.status, 'pending');
	assert.equal(row.statusLabel, 'Needs you');
	assert.equal(row.needsOwner, true);
	assert.equal(row.approvalId, '0123456789abcdef');
	assert.equal(row.when, '2 min ago');
	assert.match(row.note, /allow or deny/i);
});

test('settled requests say what happened and wait on nothing', () => {
	const rows = inboxView(
		[
			record({ correlation_id: 'a', status: 'accepted', approval_id: undefined, updated_at: NOW - 60, response: { outcome: 'accepted', version: 1, correlation_id: 'a', responder: 'x', disclosure: 'none', answer: { kind: 'delivered' } } }),
			record({ correlation_id: 'b', status: 'denied', approval_id: undefined, updated_at: NOW - 3_600, intent: 'availability', disclosure: 'personal', response: { outcome: 'denied', version: 1, correlation_id: 'b', responder: 'x', reason: 'default' } }),
		],
		NOW,
	);
	assert.equal(rows[0].statusLabel, 'Delivered');
	assert.equal(rows[0].needsOwner, false);
	assert.equal(rows[0].approvalId, null);
	assert.equal(rows[0].when, 'just now');
	assert.equal(rows[1].statusLabel, 'Denied');
	assert.equal(rows[1].label, 'ask whether you are free and what you are doing');
	assert.match(rows[1].note, /policy/i);
});

test('rows that need the owner come first, then newest first', () => {
	const rows = inboxView(
		[
			record({ correlation_id: 'old-pending', updated_at: NOW - 900 }),
			record({ correlation_id: 'new-settled', status: 'accepted', approval_id: undefined, updated_at: NOW - 10 }),
			record({ correlation_id: 'new-pending', updated_at: NOW - 30 }),
		],
		NOW,
	);
	assert.deepEqual(
		rows.map((row) => row.key.split('/')[1]),
		['new-pending', 'old-pending', 'new-settled'],
	);
	assert.equal(needsOwnerCount([record(), record({ status: 'denied' })]), 1);
	assert.equal(needsOwnerCount([]), 0);
});

test('a lapsed request is not shown and an unknown intent reads as its name', () => {
	const rows = inboxView([record({ expires_at: NOW - 1 }), record({ correlation_id: 'odd', intent: 'proposal', disclosure: 'availability' })], NOW);
	assert.equal(rows.length, 1);
	assert.equal(rows[0].label, 'propose something and see whether you are free');
});
