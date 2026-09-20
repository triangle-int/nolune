import test from 'node:test';
import assert from 'node:assert/strict';
import {
	MAX_EVIDENCE_CHARS,
	canChange,
	completionEvidence,
	describeDuration,
	dueLabel,
	eventLabel,
	isOverdue,
	lastCheckLabel,
	localDateTimeValue,
	nextCheckLabel,
	ownerLabel,
	parseLocalDateTime,
	provenanceLabel,
	refusalMessage,
	snoozeLabel,
	snoozePresets,
	sortOpen,
	statusLabel,
	upsertCommitment,
	validateSnooze,
	waitLabel,
} from '../src/lib/commitments/commitments.js';

const T0 = 1_700_000_000;
const HOUR = 3600;
const DAY = 86_400;

/** @param {Partial<import('../src/lib/commitments/commitments.js').Commitment>} over */
function commitment(over = {}) {
	return {
		version: 1,
		id: 'cmt_1700000000_0000aaaa',
		promise: 'send the photos from Saturday',
		owner: 'companion',
		status: 'active',
		dependencies: [],
		continuity_ids: [],
		provenance: { kind: 'manual' },
		snooze_count: 0,
		created_at: T0,
		updated_at: T0,
		status_changed_at: T0,
		...over,
	};
}

test('status, owner, and provenance read as plain words', () => {
	assert.equal(statusLabel('active'), 'Active');
	assert.equal(statusLabel('waiting'), 'Waiting');
	assert.equal(statusLabel('blocked'), 'Blocked');
	assert.equal(statusLabel('due'), 'Due');
	assert.equal(statusLabel('completed'), 'Completed');
	assert.equal(statusLabel('dismissed'), 'Cancelled');
	assert.equal(statusLabel('failed'), 'Failed');
	assert.equal(statusLabel('whatever'), 'Unknown');
	assert.equal(ownerLabel('companion', 'Little Moon'), 'Little Moon promised');
	assert.equal(ownerLabel('companion'), 'Nolune promised');
	assert.equal(ownerLabel('user'), 'You promised');
	assert.equal(provenanceLabel({ kind: 'manual' }), 'Added by you');
	assert.equal(provenanceLabel({ kind: 'chat', chat_id: 'default' }), 'From a conversation');
	assert.equal(provenanceLabel({ kind: 'run', run_id: 'run_1' }), 'From a check-in');
	assert.equal(canChange(commitment()), true);
	assert.equal(canChange(commitment({ status: 'due' })), true);
	assert.equal(canChange(commitment({ status: 'completed' })), false);
	assert.equal(canChange(commitment({ status: 'dismissed' })), false);
});

test('due wording comes from the deadline and a fixed clock', () => {
	assert.equal(describeDuration(30), 'under a minute');
	assert.equal(describeDuration(60), '1 minute');
	assert.equal(describeDuration(5 * 60), '5 minutes');
	assert.equal(describeDuration(HOUR), '1 hour');
	assert.equal(describeDuration(3 * HOUR + 59 * 60), '3 hours');
	assert.equal(describeDuration(DAY), '1 day');
	assert.equal(describeDuration(6 * DAY), '6 days');
	assert.equal(describeDuration(15 * DAY), '2 weeks');

	assert.equal(dueLabel(commitment(), T0), '');
	const at = commitment({ deadline: { kind: 'at', at: T0 + 2 * HOUR } });
	assert.equal(dueLabel(at, T0), 'Due in 2 hours');
	assert.equal(dueLabel(at, T0 + 2 * HOUR), 'Due now');
	assert.equal(dueLabel(at, T0 + 2 * HOUR + 30), 'Due now');
	assert.equal(dueLabel(at, T0 + 5 * HOUR), 'Overdue by 3 hours');
	assert.equal(isOverdue(at, T0), false);
	assert.equal(isOverdue(at, T0 + 5 * HOUR), true);
	// A snooze hides the overdue state until it ends; closed records are never overdue.
	assert.equal(isOverdue({ ...at, snoozed_until: T0 + 6 * HOUR }, T0 + 5 * HOUR), false);
	assert.equal(isOverdue({ ...at, status: 'completed' }, T0 + 5 * HOUR), false);

	const window = commitment({ deadline: { kind: 'window', start: T0 + DAY, end: T0 + 2 * DAY } });
	assert.equal(dueLabel(window, T0), 'Due in 1 day');
	assert.equal(dueLabel(window, T0 + DAY + HOUR), 'Due now, 23 hours left');
	assert.equal(dueLabel(window, T0 + 3 * DAY), 'Overdue by 1 day');
	assert.equal(isOverdue(window, T0 + DAY + HOUR), true);
});

test('waiting, blocked, snooze, and next-check wording never invent facts', () => {
	assert.equal(waitLabel(commitment(), T0), '');
	assert.equal(waitLabel(commitment({ waiting_on: { kind: 'until', until: T0 + 3 * HOUR } }), T0), 'Waiting 3 hours more');
	assert.equal(waitLabel(commitment({ waiting_on: { kind: 'until', until: T0 } }), T0 + 1), 'The wait is over');
	assert.equal(waitLabel(commitment({ waiting_on: { kind: 'event', event: 'machine_connected:studio-mac' } }), T0), 'Waiting for studio-mac to connect');
	assert.equal(waitLabel(commitment({ waiting_on: { kind: 'event', event: 'email_reply:thread-9' } }), T0), 'Waiting for email_reply:thread-9');
	assert.equal(waitLabel(commitment({ waiting_on: { kind: 'user_reply' } }), T0), 'Waiting for your reply');
	assert.equal(waitLabel(commitment({ dependencies: ['cmt_a'] }), T0), 'Waiting on 1 other commitment');
	assert.equal(waitLabel(commitment({ dependencies: ['cmt_a', 'cmt_b'] }), T0), 'Waiting on 2 other commitments');

	assert.equal(eventLabel('machine_connected:studio-mac'), 'studio-mac connected');
	assert.equal(eventLabel('commitment_completed:cmt_a'), 'a commitment it depended on was completed');
	assert.equal(eventLabel('email_reply:thread-9'), 'email_reply:thread-9');

	assert.equal(snoozeLabel(commitment(), T0), '');
	assert.equal(snoozeLabel(commitment({ snoozed_until: T0 + HOUR, snooze_count: 1 }), T0), 'Snoozed, 1 hour left');
	assert.equal(snoozeLabel(commitment({ snoozed_until: T0 - 1, snooze_count: 2 }), T0), 'Snoozed 2 times');

	assert.equal(nextCheckLabel(commitment(), T0), '');
	assert.equal(nextCheckLabel(commitment({ next_check: T0 + 2 * DAY }), T0), 'Next check in 2 days');
	assert.equal(nextCheckLabel(commitment({ next_check: T0 - 5 }), T0), 'Check pending');
	assert.equal(nextCheckLabel(commitment({ status: 'completed', next_check: T0 + 5 }), T0), '');
});

test('the last check says what happened and links the run', () => {
	assert.equal(lastCheckLabel(null, T0), 'Not checked yet');
	assert.equal(lastCheckLabel(undefined, T0), 'Not checked yet');
	assert.equal(lastCheckLabel({ at: T0 - 2 * HOUR, outcome: { kind: 'triggered' }, run_id: 'run_1' }, T0), 'Checked 2h ago, a check-in ran');
	assert.equal(lastCheckLabel({ at: T0 - 60, outcome: { kind: 'unchanged' } }, T0), 'Checked 1m ago, nothing changed');
	assert.equal(
		lastCheckLabel({ at: T0 - 30, outcome: { kind: 'failed', error: 'model unavailable', retryable: true }, run_id: 'run_2' }, T0),
		'Checked just now, failed and can be retried: model unavailable',
	);
	assert.equal(
		lastCheckLabel({ at: T0 - 30, outcome: { kind: 'failed', error: 'gone', retryable: false } }, T0),
		'Checked just now, failed: gone',
	);
	assert.equal(
		lastCheckLabel({ at: T0 - DAY, outcome: { kind: 'observed', event: 'machine_connected:mac' } }, T0),
		'Checked 1d ago, observed: mac connected',
	);
	assert.equal(
		lastCheckLabel({ at: T0 - DAY, outcome: { kind: 'triggered' }, pending_event: 'commitment_completed:cmt_b' }, T0),
		'Checked 1d ago, a check-in ran, still to act on: a commitment it depended on was completed',
	);
});

test('snooze presets are in the future and a custom time is validated', () => {
	const presets = snoozePresets(T0);
	assert.deepEqual(
		presets.map((p) => [p.label, p.until - T0]),
		[
			['1 hour', HOUR],
			['3 hours', 3 * HOUR],
			['Tomorrow', DAY],
			['Next week', 7 * DAY],
		],
	);
	assert.deepEqual(validateSnooze(T0 + 60, T0), { ok: true, until: T0 + 60 });
	assert.deepEqual(validateSnooze(T0, T0), { ok: false, reason: 'Pick a time in the future.' });
	assert.deepEqual(validateSnooze(Number.NaN, T0), { ok: false, reason: 'Pick a time in the future.' });
	assert.deepEqual(validateSnooze(null, T0), { ok: false, reason: 'Pick a time in the future.' });
	// datetime-local round trip is exact to the minute in the browser's zone.
	const value = localDateTimeValue(T0 + 90);
	assert.match(value, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/);
	assert.equal(parseLocalDateTime(value), T0 + 60);
	assert.equal(parseLocalDateTime(''), null);
	assert.equal(parseLocalDateTime('not a date'), null);
});

test('completion is never silent: confirmation or evidence, bounded', () => {
	assert.deepEqual(completionEvidence({ confirmed: false, summary: '' }), {
		ok: false,
		reason: 'Confirm it is done, or say how you know.',
	});
	assert.deepEqual(completionEvidence({ confirmed: false, summary: '   ' }), {
		ok: false,
		reason: 'Confirm it is done, or say how you know.',
	});
	assert.deepEqual(completionEvidence({ confirmed: true, summary: '' }), {
		ok: true,
		evidence: { confirmed_by_user: true },
	});
	assert.deepEqual(completionEvidence({ confirmed: false, summary: ' Ana replied with thanks ' }), {
		ok: true,
		evidence: { confirmed_by_user: false, summary: 'Ana replied with thanks' },
	});
	assert.deepEqual(completionEvidence({ confirmed: true, summary: 'sent at noon' }), {
		ok: true,
		evidence: { confirmed_by_user: true, summary: 'sent at noon' },
	});
	const long = 'x'.repeat(MAX_EVIDENCE_CHARS + 1);
	assert.deepEqual(completionEvidence({ confirmed: false, summary: long }), {
		ok: false,
		reason: `Keep the evidence under ${MAX_EVIDENCE_CHARS} characters.`,
	});
});

test('open commitments list due first, then active, waiting, blocked', () => {
	const due = commitment({ id: 'cmt_due', status: 'due', deadline: { kind: 'at', at: T0 - HOUR }, created_at: T0 - 10 });
	const dueLater = commitment({ id: 'cmt_due_later', status: 'due', deadline: { kind: 'window', start: T0 - 30, end: T0 + HOUR }, created_at: T0 });
	const active = commitment({ id: 'cmt_active', status: 'active', next_check: T0 + DAY, created_at: T0 - 100 });
	const activeSoon = commitment({ id: 'cmt_active_soon', status: 'active', next_check: T0 + HOUR, created_at: T0 - 200 });
	const activeNoCheck = commitment({ id: 'cmt_active_none', status: 'active', created_at: T0 });
	const waiting = commitment({ id: 'cmt_waiting', status: 'waiting', waiting_on: { kind: 'user_reply' } });
	const blocked = commitment({ id: 'cmt_blocked', status: 'blocked', dependencies: ['cmt_active'] });
	const closed = commitment({ id: 'cmt_done', status: 'completed' });
	const sorted = sortOpen([closed, blocked, active, waiting, activeNoCheck, dueLater, activeSoon, due], T0);
	assert.deepEqual(
		sorted.map((c) => c.id),
		['cmt_due', 'cmt_due_later', 'cmt_active_soon', 'cmt_active', 'cmt_active_none', 'cmt_waiting', 'cmt_blocked'],
	);
});

test('live updates replace a commitment in place and keep newest first', () => {
	const older = commitment({ id: 'cmt_old', created_at: T0 - DAY });
	const newer = commitment({ id: 'cmt_new', created_at: T0 });
	let list = upsertCommitment([older], newer);
	assert.deepEqual(list.map((c) => c.id), ['cmt_new', 'cmt_old']);
	list = upsertCommitment(list, { ...older, status: 'completed' });
	assert.equal(list.length, 2);
	assert.equal(list[1].status, 'completed');
	assert.deepEqual(upsertCommitment([], newer).map((c) => c.id), ['cmt_new']);
});

test('API refusals are explained in the user’s words', () => {
	assert.equal(refusalMessage('{"error":"evidence_required","message":"x"}'), 'Confirm it is done, or say how you know.');
	assert.equal(refusalMessage('{"error":"closed","message":"x"}'), 'This commitment is already closed.');
	assert.equal(refusalMessage('{"error":"not_found","message":"x"}'), 'This commitment no longer exists.');
	assert.equal(refusalMessage('{"error":"invalid","message":"snooze must be in the future"}'), 'snooze must be in the future');
	assert.equal(refusalMessage('{"error":"superseded","message":"x"}'), 'Something changed meanwhile. Reload and try again.');
	assert.equal(refusalMessage('{"error":"storage_error","message":"x"}'), 'Could not save that. Please try again.');
	assert.equal(refusalMessage('Internal Server Error'), 'Could not save that. Please try again.');
	assert.equal(refusalMessage(''), 'Could not save that. Please try again.');
});
