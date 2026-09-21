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
		reason: 'default',
		response: { outcome: 'needs_owner', version: 1, correlation_id: 'req-1', responder: 'x', reason: 'default' },
		approval_id: '0123456789abcdef',
		requested_at: NOW - 120,
		updated_at: NOW - 120,
		expires_at: NOW + 3_480,
		...overrides,
	};
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
		expires_at: NOW + 86_280,
		summary: '',
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
	assert.equal(row.tone, 'pending');
	assert.equal(row.statusLabel, 'Needs you');
	assert.equal(row.needsOwner, true);
	assert.equal(row.approvalId, '0123456789abcdef');
	assert.equal(row.when, '2 min ago');
	assert.match(row.note, /allow or deny/i);
	// The same with the queue in hand and the request still open.
	const [same] = inboxView([record()], NOW, [approval()]);
	assert.equal(same.needsOwner, true);
	assert.equal(same.statusLabel, 'Needs you');
});

test('a request the owner already decided no longer needs them, before the peer asks again', () => {
	const [denied] = inboxView([record()], NOW, [approval({ status: 'denied', decided_at: NOW - 30 })]);
	assert.equal(denied.needsOwner, false);
	assert.equal(denied.status, 'pending');
	assert.equal(denied.tone, 'denied');
	assert.equal(denied.statusLabel, 'Denied');
	assert.equal(denied.approvalId, null);
	assert.match(denied.note, /you denied/i);
	assert.match(denied.note, /asks again/i);
	const [allowed] = inboxView([record()], NOW, [approval({ status: 'approved', decided_at: NOW - 30, expires_at: NOW + 3_570 })]);
	assert.equal(allowed.needsOwner, false);
	assert.equal(allowed.tone, 'approved');
	assert.equal(allowed.statusLabel, 'Allowed');
	assert.match(allowed.note, /you allowed/i);
	// Another request's decision, a lapsed one, or one under another pairing says nothing about this row.
	for (const other of [
		approval({ id: 'fedcba9876543210', status: 'denied' }),
		approval({ status: 'denied', expires_at: NOW - 1 }),
		approval({ status: 'denied', pairing_id: 'another' }),
	]) {
		const [row] = inboxView([record()], NOW, [other]);
		assert.equal(row.needsOwner, true, JSON.stringify(other));
		assert.equal(row.statusLabel, 'Needs you');
	}
});

test('settled requests say what happened, why, and wait on nothing', () => {
	const rows = inboxView(
		[
			record({ correlation_id: 'a', status: 'accepted', reason: 'rule', approval_id: undefined, updated_at: NOW - 60, response: { outcome: 'accepted', version: 1, correlation_id: 'a', responder: 'x', disclosure: 'none', answer: { kind: 'delivered' } } }),
			record({ correlation_id: 'b', status: 'denied', reason: 'default', approval_id: undefined, updated_at: NOW - 3_600, intent: 'availability', disclosure: 'personal', response: { outcome: 'denied', version: 1, correlation_id: 'b', responder: 'x', reason: 'default' } }),
			record({ correlation_id: 'c', status: 'denied', reason: 'owner_denied', approval_id: undefined, updated_at: NOW - 7_200 }),
		],
		NOW,
	);
	assert.equal(rows[0].statusLabel, 'Delivered');
	assert.equal(rows[0].tone, 'accepted');
	assert.equal(rows[0].needsOwner, false);
	assert.equal(rows[0].approvalId, null);
	assert.equal(rows[0].when, 'just now');
	assert.equal(rows[1].statusLabel, 'Denied');
	assert.equal(rows[1].tone, 'denied');
	assert.equal(rows[1].label, 'ask whether you are free and what you are doing');
	assert.match(rows[1].note, /policy/i);
	// Denied by the owner once: the wire still said needs_owner, the row says who refused it.
	assert.equal(rows[2].statusLabel, 'Denied');
	assert.equal(rows[2].tone, 'denied');
	assert.equal(rows[2].needsOwner, false);
	assert.match(rows[2].note, /you denied this once/i);
	assert.doesNotMatch(rows[2].note, /policy/i);
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
	assert.equal(needsOwnerCount([record(), record({ status: 'denied' })], NOW), 1);
	assert.equal(needsOwnerCount([record()], NOW, [approval({ status: 'denied' })]), 0);
	assert.equal(needsOwnerCount([], NOW), 0);
});

test('a lapsed request is not shown and an unknown intent reads as its name', () => {
	const rows = inboxView([record({ expires_at: NOW - 1 }), record({ correlation_id: 'odd', intent: 'proposal', disclosure: 'availability' })], NOW);
	assert.equal(rows.length, 1);
	assert.equal(rows[0].label, 'propose something and see whether you are free');
});
