import test from 'node:test';
import assert from 'node:assert/strict';
import {
	BREAK_OPTIONS,
	COOLDOWN_OPTIONS,
	applyResumeEvent,
	handoffAnchor,
	heldMessage,
	optionLabel,
	reviewHref,
	ritualSummary,
	snoozePresets,
	snoozeStatus,
	suggestedLabel,
} from '../src/lib/continuity/resume.js';

const NOW = 1_767_607_200;

const card = {
	record_id: 'task_1',
	goal: 'rename the trip photos',
	state: 'active',
	origin_chat_id: 'chat_1',
	origin: null,
	completed_steps: [],
	resources: [],
	blockers: [],
	next_step: 'rename IMG_* files',
	required: { capabilities: [], permissions: [] },
	decision: null,
	bound_to: null,
	offered: true,
	created_at: NOW - 3600,
	updated_at: NOW - 600,
};

const offer = {
	id: 'sug_1',
	record_id: 'task_1',
	goal: 'rename the trip photos',
	trigger: { kind: 'opened_after_break', away_secs: 7200 },
	why_now: 'You opened Nolune after 2 hours away.',
	why_this: 'high priority, due in 3 hours, studio can take it',
	destination_id: 'mac-a',
	suggested_at: NOW - 120,
	card,
};

test('a resume_updated event replaces or clears the offer', () => {
	assert.equal(applyResumeEvent(null, { type: 'resume_updated', instance_slug: 'companion', suggestion: offer }), offer);
	assert.equal(applyResumeEvent(offer, { type: 'resume_updated', instance_slug: 'companion', suggestion: null }), null);
});

test('a decided or withdrawn card resolves the offer; an updated card is folded in', () => {
	const kept = { ...card, decision: { kind: 'kept', at: NOW }, offered: false };
	assert.equal(applyResumeEvent(offer, { type: 'handoff_updated', instance_slug: 'companion', card: kept }), null);
	const accepted = { ...card, decision: { kind: 'accepted', machine_id: 'mac-a', run_id: 'run_1', at: NOW, outcome: null } };
	assert.equal(applyResumeEvent(offer, { type: 'handoff_updated', instance_slug: 'companion', card: accepted }), null);
	const progressed = { ...card, completed_steps: ['listed the folder'], updated_at: NOW };
	const next = applyResumeEvent(offer, { type: 'handoff_updated', instance_slug: 'companion', card: progressed });
	assert.deepEqual(next, { ...offer, card: progressed });
	// Another record's card, or an unrelated event, changes nothing.
	const other = { ...card, record_id: 'task_2', offered: false };
	assert.equal(applyResumeEvent(offer, { type: 'handoff_updated', instance_slug: 'companion', card: other }), undefined);
	assert.equal(applyResumeEvent(offer, { type: 'mood_updated', instance_slug: 'companion', mood: 'calm' }), undefined);
	assert.equal(applyResumeEvent(null, { type: 'handoff_updated', instance_slug: 'companion', card: kept }), undefined);
});

test('review and continue links to an anchor of its own, never the card heading id', () => {
	// HandoffCard names its <article> by the heading `handoff-<record_id>`
	// (aria-labelledby); the anchor the list item carries must be another id,
	// or the first match in tree order would rename the card to its whole text.
	assert.equal(handoffAnchor('task_1'), 'handoff-card-task_1');
	assert.notEqual(handoffAnchor('task_1'), 'handoff-task_1');
	assert.equal(reviewHref('companion', 'task_1'), '/companion/activity#handoff-card-task_1');
});

test('the banner states when the suggestion was made', () => {
	assert.equal(suggestedLabel(offer, NOW), 'Suggested 2m ago');
	assert.equal(suggestedLabel({ ...offer, suggested_at: NOW }, NOW), 'Suggested just now');
});

test('a held reason reads as one sentence', () => {
	assert.equal(heldMessage({ kind: 'nothing_to_resume' }, NOW), 'Nothing to resume right now.');
	assert.equal(heldMessage({ kind: 'quiet_hours' }, NOW), 'Quiet hours are on, so nothing is suggested now.');
	assert.equal(heldMessage({ kind: 'cooldown', until: NOW + 1800 }, NOW), 'A suggestion was made or refused recently; the next one can come in 30 minutes.');
	assert.equal(heldMessage({ kind: 'snoozed', until: NOW + 7200 }, NOW), 'Resume my work is snoozed for 2 hours.');
	assert.equal(heldMessage({ kind: 'disabled' }, NOW), 'Resume my work is off. Turn it on under Settings.');
	assert.equal(heldMessage({ kind: 'no_break' }, NOW), 'No break long enough has passed.');
	assert.equal(heldMessage(undefined, NOW), 'Nothing to resume right now.');
});

test('snooze presets are fixed offsets and the status says how long is left', () => {
	const presets = snoozePresets(NOW);
	assert.deepEqual(
		presets.map((p) => [p.label, p.until - NOW]),
		[
			['1 hour', 3600],
			['4 hours', 4 * 3600],
			['Tomorrow', 86400],
		],
	);
	assert.equal(snoozeStatus({ snooze_until: null }, NOW), '');
	assert.equal(snoozeStatus({ snooze_until: NOW - 1 }, NOW), '');
	assert.equal(snoozeStatus({ snooze_until: NOW + 5400 }, NOW), 'Snoozed, 1 hour left');
});

test('settings options resolve stored values, including ones not in the list', () => {
	assert.equal(optionLabel(BREAK_OPTIONS, 120), '2 hours');
	assert.equal(optionLabel(BREAK_OPTIONS, 45), '45 minutes');
	assert.equal(optionLabel(COOLDOWN_OPTIONS, 3600), '1 hour');
	assert.equal(optionLabel(COOLDOWN_OPTIONS, 0), 'None');
	assert.equal(optionLabel(COOLDOWN_OPTIONS, 5400), '90 minutes');
	assert.ok(BREAK_OPTIONS.every((o) => Number.isInteger(o.value) && o.value > 0));
});

test('the ritual summary names the break, the cooldown, and quiet hours', () => {
	const policy = { enabled: true, break_minutes: 120, cooldown_secs: 3600, snooze_until: null, dismissed_record_ids: [] };
	assert.equal(
		ritualSummary(policy, false),
		'After 2 hours away, or when a computer a task names reconnects, at most one suggestion per hour.',
	);
	assert.equal(
		ritualSummary(policy, true),
		'After 2 hours away, or when a computer a task names reconnects, at most one suggestion per hour. Held during quiet hours.',
	);
	assert.equal(ritualSummary({ ...policy, enabled: false }, true), 'Off. Nothing is suggested until you turn it on.');
	assert.equal(
		ritualSummary({ ...policy, cooldown_secs: 0 }, false),
		'After 2 hours away, or when a computer a task names reconnects, at most one suggestion per trigger.',
	);
});
