import test from 'node:test';
import assert from 'node:assert/strict';
import {
	canCancel,
	canRetry,
	outboxLabel,
	outboxNote,
	outboxStatusLabel,
	outboxText,
	outcomeSummary,
	relativeTime,
	runTargetLabel,
	statusLabel,
	targetLabel,
	triggerLabel,
	upsertOutboxEntry,
	upsertRun,
} from '../src/lib/activity/receipts.js';

const base = {
	id: 'run_100_a',
	trigger: { kind: 'heartbeat', agent: 'companion' },
	reason: 'periodic check-in',
	target: { kind: 'companion' },
	status: { kind: 'completed' },
	attempt: 1,
	started_at: 100,
	finished_at: 105,
	approvals: [],
	outcome: { actions: [{ tool: 'reach_out', summary: 'sent a message' }, { tool: 'memory_write', summary: 'writing notes/tea.md' }], messages_sent: 1, tokens: 10 },
};

test('receipts describe trigger and status without exposing model text', () => {
	assert.equal(triggerLabel({ kind: 'heartbeat', agent: 'companion' }), 'Check-in');
	assert.equal(triggerLabel({ kind: 'heartbeat', agent: 'reflection' }), 'Reflection');
	assert.equal(triggerLabel({ kind: 'machine_connected', machine_id: 'mac' }), 'Computer connected · mac');
	assert.equal(statusLabel({ kind: 'skipped', reason: { kind: 'quiet_hours' } }), 'Held for quiet hours');
	assert.equal(statusLabel({ kind: 'skipped', reason: { kind: 'import' } }), 'Held for an import');
	assert.equal(statusLabel({ kind: 'failed', error: 'x', retryable: true }), 'Failed, can retry');
	assert.equal(statusLabel({ kind: 'failed', error: 'x', retryable: false }), 'Failed');
	assert.equal(outcomeSummary(base), 'sent you a message, 1 action');
	assert.equal(outcomeSummary({ ...base, outcome: { actions: [], messages_sent: 0, tokens: 0 } }), 'nothing to do');
	assert.equal(outcomeSummary({ ...base, outcome: null }), '');
});

test('cancel and retry follow the run status exactly', () => {
	assert.equal(canCancel({ ...base, status: { kind: 'running' } }), true);
	assert.equal(canCancel(base), false);
	assert.equal(canRetry({ ...base, status: { kind: 'cancelled' } }), true);
	assert.equal(canRetry({ ...base, status: { kind: 'failed', error: 'x', retryable: true } }), true);
	assert.equal(canRetry({ ...base, status: { kind: 'failed', error: 'x', retryable: false } }), false);
	assert.equal(canRetry(base), false);
});

test('live updates replace runs in place and keep newest first', () => {
	const older = { ...base, id: 'run_50_b', started_at: 50, status: { kind: 'running' } };
	let runs = upsertRun([base], older);
	assert.deepEqual(runs.map((r) => r.id), ['run_100_a', 'run_50_b']);
	runs = upsertRun(runs, { ...older, status: { kind: 'completed' } });
	assert.equal(runs.length, 2);
	assert.equal(runs[1].status.kind, 'completed');
	assert.equal(relativeTime(1000, 1030), 'just now');
	assert.equal(relativeTime(1000, 1000 + 120), '2m ago');
	assert.equal(relativeTime(1000, 1000 + 7200), '2h ago');
	assert.equal(relativeTime(1000, 1000 + 3 * 86400), '3d ago');
});

test('the target of a run is named the way the Computers tab names it (#80)', () => {
	const machines = [{ machine_id: 'studio-id', display_name: 'Studio Mac' }];
	assert.equal(targetLabel({ kind: 'machine', machine_id: 'studio-id' }, machines), 'On Studio Mac');
	assert.equal(targetLabel({ kind: 'machine', machine_id: 'laptop-id' }, machines), 'On laptop-id');
	assert.equal(targetLabel({ kind: 'machine', machine_id: 'laptop-id' }), 'On laptop-id');
	assert.equal(targetLabel({ kind: 'companion' }, machines), '');
	assert.equal(targetLabel({ kind: 'chat', chat_id: 'default' }, machines), '');
	assert.equal(triggerLabel({ kind: 'machine_connected', machine_id: 'studio-id' }, machines), 'Computer connected · Studio Mac');
	assert.equal(triggerLabel({ kind: 'machine_connected', machine_id: 'studio-id' }), 'Computer connected · studio-id');
	// A card never names the same computer twice.
	const connected = { ...base, trigger: { kind: 'machine_connected', machine_id: 'studio-id' }, target: { kind: 'machine', machine_id: 'studio-id' } };
	assert.equal(runTargetLabel(connected, machines), '');
	const handoff = { ...base, trigger: { kind: 'handoff', handoff_id: 'h1' }, target: { kind: 'machine', machine_id: 'studio-id' } };
	assert.equal(runTargetLabel(handoff, machines), 'On Studio Mac');
	assert.equal(runTargetLabel(base, machines), '');
});

// Requests sent to paired companions (#110): the outbox entry is the only
// source. The text shown is this owner's own; the peer's typed response
// carries ids, classes, times, and spans, never words.
const PEER = 'TFccHElqXR1lkUBqQoQPWYxPSm1wPjCl8WXbgm_cQ7E';
const outboxBase = {
	version: 1,
	recipient: PEER,
	pairing_id: '9f1c0b7e2a6d4c31',
	intent: {
		version: 1,
		correlation_id: 'c0ffee',
		sender: 'yob7BCJNccQgNdznxaQZ_Rxn1k5ACU12hxmKeBKNhL0',
		represented_owner: 'Bob',
		purpose: 'plans for the weekend',
		disclosure: 'none',
		issued_at: 1000,
		expires_at: 1000 + 86400,
		intent: { type: 'message', body: 'see you on Friday at the lake' },
	},
	status: 'queued',
	attempts: [],
	next_attempt_at: 1000,
	chat_id: 'default',
	created_at: 1000,
	updated_at: 1000,
};

test('outbox rows name the request kind and the companion, never its words', () => {
	assert.equal(outboxLabel(outboxBase), 'Message to companion TFccHElq…cQ7E');
	assert.equal(outboxLabel({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'availability', window: { from: 2000, to: 9200 } } } }), 'Availability asked of companion TFccHElq…cQ7E');
	assert.equal(outboxLabel({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'reminder', text: 'water the plants', at: 5000 } } }), 'Reminder proposed to companion TFccHElq…cQ7E');
	assert.equal(outboxLabel({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'proposal', description: 'x', window: { from: 1, to: 2 } } } }), 'Meeting proposed to companion TFccHElq…cQ7E');
	assert.equal(outboxLabel({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'handoff', task: { record_id: 'task_1', goal: 'Print the zine', completed_steps: [], blockers: [], resources: [], provenance: [] } } } }), 'Task handed over to companion TFccHElq…cQ7E');
	assert.equal(outboxLabel({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'ping' } } }), 'Request to companion TFccHElq…cQ7E');
	assert.equal(outboxText(outboxBase), 'see you on Friday at the lake');
	assert.equal(outboxText({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'reminder', text: 'water the plants', at: 5000 } } }), 'water the plants');
	const window = outboxText({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'availability', window: { from: 1_800_090_000, to: 1_800_097_200 } } } });
	assert.match(window, /^Free between .+ and .+\?$/);
	assert.ok(!window.includes('undefined'));
	const meeting = outboxText({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'proposal', description: 'lunch by the lake', window: { from: 1_800_090_000, to: 1_800_097_200 } } } });
	assert.match(meeting, /^lunch by the lake\nBetween .+ and .+$/);
	const handoff = outboxText({ ...outboxBase, intent: { ...outboxBase.intent, intent: { type: 'handoff', task: { record_id: 'task_1', goal: 'Print the zine', completed_steps: ['a', 'b'], blockers: [], resources: [], provenance: [] } } } });
	assert.equal(handoff, 'Print the zine\n2 steps done so far');
});

test('outbox status words follow the entry and count attempts', () => {
	assert.equal(outboxStatusLabel(outboxBase), 'Queued');
	const tried = { ...outboxBase, attempts: [{ at: 1000, outcome: { kind: 'unreachable' } }], next_attempt_at: 1030, updated_at: 1000 };
	assert.equal(outboxStatusLabel(tried), 'Not delivered yet');
	assert.equal(outboxStatusLabel({ ...outboxBase, status: 'waiting_owner' }), 'Waiting for their owner');
	assert.equal(outboxStatusLabel({ ...outboxBase, status: 'delivered' }), 'Delivered');
	assert.equal(outboxStatusLabel({ ...outboxBase, status: 'denied' }), 'Refused');
	assert.equal(outboxStatusLabel({ ...outboxBase, status: 'failed' }), 'Failed');
	assert.equal(outboxStatusLabel({ ...outboxBase, status: 'expired' }), 'Expired');
	assert.equal(outboxStatusLabel({ ...outboxBase, status: 'something_else' }), 'Unknown');
});

test('outbox notes say what happened, how often it was tried, and what comes next', () => {
	assert.equal(outboxNote(outboxBase, 1000), 'Sending now.');
	const tried = { ...outboxBase, attempts: [{ at: 1000, outcome: { kind: 'unreachable' } }, { at: 1030, outcome: { kind: 'unreachable' } }], next_attempt_at: 1030 + 60, updated_at: 1030 };
	assert.equal(outboxNote(tried, 1040), 'Could not reach it, 2 attempts · next try in 50s.');
	assert.equal(outboxNote({ ...tried, attempts: [tried.attempts[0]], next_attempt_at: 1030 }, 1000), 'Could not reach it, 1 attempt · next try in 30s.');
	assert.equal(outboxNote({ ...tried, next_attempt_at: 900 }, 1000), 'Could not reach it, 2 attempts · next try now.');
	const interrupted = { ...tried, attempts: [{ at: 1000, outcome: { kind: 'interrupted' } }], next_attempt_at: 1100 };
	assert.equal(outboxNote(interrupted, 1000), 'Could not reach it, 1 attempt · next try in 1m.');
	const refused = { ...tried, attempts: [{ at: 1000, outcome: { kind: 'refused', status: 429, code: 'rate_limited' } }], next_attempt_at: 1060 };
	assert.equal(outboxNote(refused, 1000), 'Their companion refused for now (rate_limited), 1 attempt · next try in 1m.');
	assert.equal(outboxNote({ ...refused, attempts: [{ at: 1000, outcome: { kind: 'refused', code: 'peer_not_paired' } }] }, 1000), 'Their companion refused for now (peer_not_paired), 1 attempt · next try in 1m.');
	// A proxy's or a tunnel's own error page in front of the companion is
	// named by its status, not read as the companion's word.
	const inFront = { ...tried, attempts: [{ at: 1000, outcome: { kind: 'unreachable' } }, { at: 1030, outcome: { kind: 'refused', status: 502, code: 'unknown' } }], next_attempt_at: 1090 };
	assert.equal(outboxNote(inFront, 1040), 'Something in front of it answered HTTP 502, 2 attempts · next try in 50s.');
	const overLimit = { ...tried, attempts: [{ at: 1000, outcome: { kind: 'answered', outcome: 'denied', reason: 'rate_limited' } }], next_attempt_at: 1045, response: { outcome: 'denied', version: 1, correlation_id: 'c0ffee', responder: PEER, reason: 'rate_limited', retry_after_secs: 45 } };
	assert.equal(outboxNote(overLimit, 1000), 'Their companion refused for now (rate_limited), 1 attempt · next try in 45s.');
	const waiting = { ...outboxBase, status: 'waiting_owner', attempts: [{ at: 1000, outcome: { kind: 'answered', outcome: 'needs_owner', reason: 'default' } }], next_attempt_at: 1900, response: { outcome: 'needs_owner', version: 1, correlation_id: 'c0ffee', responder: PEER, reason: 'default' }, updated_at: 1000 };
	assert.equal(outboxNote(waiting, 1000), 'Their owner has to allow it first · asks again in 15m.');
	assert.equal(outboxNote({ ...waiting, response: { ...waiting.response, reason: 'quiet_hours' } }, 1000), 'Held during their quiet hours · asks again in 15m.');
	const delivered = { ...outboxBase, status: 'delivered', attempts: [{ at: 1000, outcome: { kind: 'answered', outcome: 'accepted' } }], response: { outcome: 'accepted', version: 1, correlation_id: 'c0ffee', responder: PEER, disclosure: 'none', answer: { kind: 'delivered' } }, updated_at: 1000 };
	assert.equal(outboxNote(delivered, 1300), 'Delivered to their conversation 5m ago.');
	const reminder = { ...delivered, intent: { ...outboxBase.intent, intent: { type: 'reminder', text: 'water the plants', at: 1_800_014_400 } }, response: { ...delivered.response, answer: { kind: 'reminder_scheduled', at: 1_800_014_400 } } };
	assert.match(outboxNote(reminder, 1300), /^Reminder for .+ delivered for their owner to accept · 5m ago\.$/);
	const handedOver = { ...delivered, response: { ...delivered.response, answer: { kind: 'handoff_received' } } };
	assert.equal(outboxNote(handedOver, 1300), 'Delivered for their owner to accept as a task of their own 5m ago.');
	const proposed = { ...delivered, response: { ...delivered.response, answer: { kind: 'proposal_received' } } };
	assert.equal(outboxNote(proposed, 1300), 'Delivered for their owner to accept 5m ago.');
	const availability = { ...delivered, intent: { ...outboxBase.intent, disclosure: 'availability', intent: { type: 'availability', window: { from: 2000, to: 9200 } } }, response: { ...delivered.response, answer: { kind: 'availability', windows: [] } } };
	assert.equal(outboxNote(availability, 1300), 'Answered 5m ago: nothing about their schedule was shared.');
	const spans = { ...availability, response: { ...availability.response, disclosure: 'availability', answer: { kind: 'availability', windows: [{ from: 2000, to: 5600, state: 'free' }, { from: 5600, to: 9200, state: 'busy' }] } } };
	assert.equal(outboxNote(spans, 1300), 'Answered 5m ago: 1 free span, 1 busy span.');
	const denied = { ...outboxBase, status: 'denied', attempts: [{ at: 1000, outcome: { kind: 'answered', outcome: 'denied', reason: 'rule' } }], response: { outcome: 'denied', version: 1, correlation_id: 'c0ffee', responder: PEER, reason: 'rule' }, updated_at: 1000 };
	assert.equal(outboxNote(denied, 1300), 'Refused by a rule their owner set · 5m ago.');
	assert.equal(outboxNote({ ...denied, response: { ...denied.response, reason: 'default' } }, 1300), 'Refused by their defaults · 5m ago.');
	assert.equal(outboxNote({ ...denied, response: { ...denied.response, reason: 'owner_denied' } }, 1300), 'Refused by their owner · 5m ago.');
	assert.equal(outboxNote({ ...denied, response: { ...denied.response, reason: 'peer_revoked' } }, 1300), 'Refused (peer_revoked) · 5m ago.');
	const failed = { ...outboxBase, status: 'failed', attempts: Array.from({ length: 16 }, (_, i) => ({ at: 900 + i, outcome: { kind: 'unreachable' } })), updated_at: 1000 };
	assert.equal(outboxNote(failed, 1300), 'Could not reach it after 16 attempts; nothing was delivered · 5m ago.');
	const refusedForGood = { ...failed, attempts: [{ at: 1000, outcome: { kind: 'refused', status: 403, code: 'sender_mismatch' } }], updated_at: 1000 };
	assert.equal(outboxNote(refusedForGood, 1300), 'Their companion refused it (sender_mismatch); nothing was delivered · 5m ago.');
	const behindAProxy = { ...failed, attempts: [...failed.attempts.slice(0, 15), { at: 1000, outcome: { kind: 'refused', status: 503, code: 'unknown' } }], updated_at: 1000 };
	assert.equal(outboxNote(behindAProxy, 1300), 'Could not reach it after 16 attempts, the last answered by HTTP 503 from in front of it; nothing was delivered · 5m ago.');
	const overLimitForGood = { ...failed, attempts: Array.from({ length: 16 }, (_, i) => ({ at: 900 + i, outcome: { kind: 'answered', outcome: 'denied', reason: 'rate_limited' } })), response: { outcome: 'denied', version: 1, correlation_id: 'c0ffee', responder: PEER, reason: 'rate_limited', retry_after_secs: 45 }, updated_at: 1000 };
	assert.equal(outboxNote(overLimitForGood, 1300), 'Their companion was over its limit for 16 attempts in a row; nothing was delivered · 5m ago.');
	const expired = { ...failed, status: 'expired', attempts: failed.attempts.slice(0, 3), updated_at: 1000 };
	assert.equal(outboxNote(expired, 1300), 'Expired before it could be delivered, 3 attempts · 5m ago.');
	// Expired while the companion kept answering: the note says what it
	// last said, never that it could not be reached.
	const expiredWaiting = { ...expired, attempts: [{ at: 900, outcome: { kind: 'answered', outcome: 'needs_owner', reason: 'default' } }, { at: 1800, outcome: { kind: 'answered', outcome: 'needs_owner', reason: 'default' } }], response: { outcome: 'needs_owner', version: 1, correlation_id: 'c0ffee', responder: PEER, reason: 'default' } };
	assert.equal(outboxNote(expiredWaiting, 1300), 'Expired before their owner allowed it, 2 attempts · 5m ago.');
	assert.equal(outboxNote({ ...expiredWaiting, response: { ...expiredWaiting.response, reason: 'quiet_hours' } }, 1300), 'Expired while held during their quiet hours, 2 attempts · 5m ago.');
	assert.equal(outboxNote({ ...expiredWaiting, response: overLimitForGood.response }, 1300), 'Expired while their companion was over its limit, 2 attempts · 5m ago.');
});

test('live outbox updates replace entries by request id and keep newest first', () => {
	const older = { ...outboxBase, intent: { ...outboxBase.intent, correlation_id: 'older' }, updated_at: 500 };
	let entries = upsertOutboxEntry([outboxBase], older);
	assert.deepEqual(entries.map((e) => e.intent.correlation_id), ['c0ffee', 'older']);
	entries = upsertOutboxEntry(entries, { ...older, status: 'delivered', updated_at: 2000 });
	assert.deepEqual(entries.map((e) => e.intent.correlation_id), ['older', 'c0ffee']);
	assert.equal(entries.length, 2);
	assert.equal(entries[0].status, 'delivered');
});
