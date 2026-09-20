import test from 'node:test';
import assert from 'node:assert/strict';
import { commitmentConditionLabel, commitmentReceipt, triggerLabel } from '../src/lib/activity/receipts.js';

const run = {
	id: 'run_200_c',
	trigger: { kind: 'commitment', commitment_id: 'cmt_1700000000_0000aaaa' },
	reason: 'deadline arrived: send the photos from Saturday',
	target: { kind: 'companion' },
	status: { kind: 'completed' },
	attempt: 1,
	started_at: 200,
	approvals: [],
	outcome: null,
};

test('a commitment check-in links back to its commitment and states the trigger condition', () => {
	assert.equal(triggerLabel(run.trigger), 'Commitment');
	assert.deepEqual(commitmentReceipt(run), {
		commitmentId: 'cmt_1700000000_0000aaaa',
		condition: 'deadline arrived',
		promise: 'send the photos from Saturday',
	});
	// The promise may itself contain a colon; only the leading condition is split off.
	assert.deepEqual(commitmentReceipt({ ...run, reason: 'event observed: reply to Ana: photos' }), {
		commitmentId: 'cmt_1700000000_0000aaaa',
		condition: 'event observed',
		promise: 'reply to Ana: photos',
	});
	// An unknown reason shape is kept whole rather than guessed at.
	assert.deepEqual(commitmentReceipt({ ...run, reason: 'something else entirely' }), {
		commitmentId: 'cmt_1700000000_0000aaaa',
		condition: '',
		promise: 'something else entirely',
	});
	assert.equal(commitmentReceipt({ ...run, trigger: { kind: 'heartbeat', agent: 'companion' } }), null);
});

test('every trigger condition explains what changed and why now', () => {
	assert.equal(commitmentConditionLabel('deadline arrived'), 'Its deadline arrived');
	assert.equal(commitmentConditionLabel('snooze ended'), 'The snooze you asked for ended');
	assert.equal(commitmentConditionLabel('wait ended'), 'The wait it was on ended');
	assert.equal(commitmentConditionLabel('event observed'), 'Something it was waiting for happened');
	assert.equal(commitmentConditionLabel('dependency completed'), 'A commitment it depended on was completed');
	assert.equal(commitmentConditionLabel('scheduled check'), 'A scheduled check came up; nothing else changed');
	assert.equal(commitmentConditionLabel(''), 'It looked at this commitment');
});
