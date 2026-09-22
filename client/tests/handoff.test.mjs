import test from 'node:test';
import assert from 'node:assert/strict';
import {
	cardActions,
	decisionLabel,
	groupChecks,
	machineChoices,
	originCopy,
	requiredSummary,
	resolveHere,
	upsertCard,
} from '../src/lib/continuity/handoff.js';

const NOW = 1_767_607_200;

const studio = {
	machine_id: 'mac-a',
	display_name: 'studio',
	known: true,
	online: true,
	health: 'healthy',
	platform: 'macos',
	last_seen: NOW - 5,
};

const card = {
	record_id: 'task_1',
	goal: 'rename the trip photos',
	state: 'active',
	origin_chat_id: 'chat_1',
	origin: studio,
	completed_steps: ['listed the folder'],
	resources: [{ resource: { kind: 'memory', path: 'notes/trip.md' }, label: 'memory notes/trip.md', available: true }],
	blockers: [],
	next_step: 'rename IMG_* files',
	required: { capabilities: ['file_read', 'file_list'], permissions: ['screen_capture', 'accessibility'] },
	decision: null,
	bound_to: null,
	offered: true,
	created_at: NOW - 600,
	updated_at: NOW - 60,
};

const machine = (id, name, online = true, health = 'healthy') => ({
	machine_id: id,
	display_name: name,
	custom_name: null,
	hostname: name,
	os: 'macos',
	platform: 'macos',
	location: 'desktop',
	permissions: null,
	capabilities: [],
	first_seen: NOW - 1000,
	last_seen: online ? NOW : NOW - 7200,
	instance_slug: 'companion',
	online,
	health,
	driver_version: null,
	cua_health: null,
});

test('the origin line stays useful when the origin computer is offline', () => {
	assert.equal(originCopy(card, NOW), 'Started on studio');
	const offline = { ...card, origin: { ...studio, online: false, health: 'unavailable', last_seen: NOW - 7200 } };
	assert.equal(originCopy(offline, NOW), 'Started on studio · offline, last seen 2h ago');
	const unknown = { ...card, origin: { ...studio, machine_id: 'ghost', display_name: 'ghost', known: false, online: false, health: 'unavailable', last_seen: null } };
	assert.equal(originCopy(unknown, NOW), 'Started on ghost · not connected to this companion');
	const stale = { ...card, origin: { ...studio, health: 'degraded' } };
	assert.equal(originCopy(stale, NOW), 'Started on studio · connected but not responding');
	assert.equal(originCopy({ ...card, origin: null }, NOW), 'Started in chat, no computer yet');
});

test('the decision line names the computer and the outcome, never model text', () => {
	assert.equal(decisionLabel(card), '');
	const laptop = { ...studio, machine_id: 'mac-b', display_name: 'Travel Laptop' };
	const accepted = { ...card, decision: { kind: 'accepted', machine_id: 'mac-b', run_id: 'run_1', at: NOW, outcome: null }, bound_to: laptop };
	assert.equal(decisionLabel(accepted), 'Continuing on Travel Laptop');
	const done = { ...accepted, decision: { ...accepted.decision, outcome: { status: 'completed', finished_at: NOW, summary: '2 actions' } } };
	assert.equal(decisionLabel(done), 'Continued on Travel Laptop · 2 actions');
	const failed = { ...accepted, decision: { ...accepted.decision, outcome: { status: 'failed', finished_at: NOW, summary: 'request timed out' } } };
	assert.equal(decisionLabel(failed), 'Stopped on Travel Laptop · request timed out');
	const cancelled = { ...accepted, decision: { ...accepted.decision, outcome: { status: 'cancelled', finished_at: NOW, summary: 'cancelled' } } };
	assert.equal(decisionLabel(cancelled), 'Cancelled on Travel Laptop');
	assert.equal(decisionLabel({ ...card, decision: { kind: 'kept', machine_id: 'mac-a', at: NOW } }), 'Kept on studio');
	assert.equal(decisionLabel({ ...card, decision: { kind: 'kept', machine_id: null, at: NOW } }), 'Kept where it is');
	assert.equal(decisionLabel({ ...card, decision: { kind: 'dismissed', at: NOW } }), 'Dismissed');
});

test('actions follow the decision: a running continuation cannot be started twice', () => {
	assert.deepEqual(cardActions(card), { continueHere: true, continueOn: true, keepThere: true, dismiss: true, continuing: false });
	const running = { ...card, decision: { kind: 'accepted', machine_id: 'mac-b', run_id: 'run_1', at: NOW, outcome: null } };
	assert.deepEqual(cardActions(running), { continueHere: false, continueOn: false, keepThere: false, dismiss: false, continuing: true });
	const finished = { ...running, decision: { ...running.decision, outcome: { status: 'failed', finished_at: NOW, summary: 'no model turn ran' } } };
	assert.deepEqual(cardActions(finished), { continueHere: true, continueOn: true, keepThere: true, dismiss: true, continuing: false });
	// A server restart closes the acceptance with the run's own failure;
	// the card offers the task again instead of waiting on a dead run.
	const interrupted = { ...running, decision: { ...running.decision, outcome: { status: 'failed', finished_at: NOW, summary: 'interrupted by server restart' } } };
	assert.deepEqual(cardActions(interrupted), { continueHere: true, continueOn: true, keepThere: true, dismiss: true, continuing: false });
	assert.equal(decisionLabel({ ...interrupted, bound_to: { ...studio, machine_id: 'mac-b', display_name: 'Travel Laptop' } }), 'Stopped on Travel Laptop · interrupted by server restart');
	const closed = { ...card, state: 'completed', offered: false };
	assert.deepEqual(cardActions(closed), { continueHere: false, continueOn: false, keepThere: false, dismiss: false, continuing: false });
});

test('machine choices put ready computers first and say why the others are not', () => {
	const choices = machineChoices(
		[machine('mac-c', 'old-mini', false), machine('mac-b', 'Travel Laptop'), machine('mac-a', 'studio'), machine('mac-d', 'spare', true, 'degraded')],
		'mac-a',
	);
	assert.deepEqual(
		choices.map((c) => [c.machine_id, c.label, c.available]),
		[
			['mac-a', 'studio · where it started', true],
			['mac-b', 'Travel Laptop', true],
			['mac-d', 'spare · not responding', false],
			['mac-c', 'old-mini · offline', false],
		],
	);
	assert.deepEqual(machineChoices([], 'mac-a'), []);
});

test('"here" is the remembered computer when it is connected, else the only connected one', () => {
	const machines = [machine('mac-a', 'studio'), machine('mac-b', 'Travel Laptop', false)];
	assert.equal(resolveHere(machines, 'mac-b'), null, 'a remembered offline computer is not here');
	assert.equal(resolveHere(machines, null), 'mac-a', 'the only connected computer');
	assert.equal(resolveHere([machine('mac-a', 'studio'), machine('mac-b', 'Travel Laptop')], 'mac-b'), 'mac-b');
	assert.equal(resolveHere([machine('mac-a', 'studio'), machine('mac-b', 'Travel Laptop')], null), null, 'two connected: ask');
	assert.equal(resolveHere([], null), null);
});

test('checks are grouped by what they mean for the user', () => {
	const grouped = groupChecks([
		{ kind: { kind: 'machine_offline' }, severity: 'blocking', detail: 'Travel Laptop is offline' },
		{ kind: { kind: 'permission_prompt', permission: 'screen_capture' }, severity: 'approval', detail: 'Travel Laptop will ask for Screen Recording' },
		{ kind: { kind: 'resource_elsewhere', resource: { kind: 'machine_path', machine_id: 'mac-a', path: '/Volumes/Trip' }, machine_id: 'mac-a' }, severity: 'note', detail: '/Volumes/Trip is on studio' },
	]);
	assert.deepEqual(grouped.blocking, ['Travel Laptop is offline']);
	assert.deepEqual(grouped.approvals, ['Travel Laptop will ask for Screen Recording']);
	assert.deepEqual(grouped.notes, ['/Volumes/Trip is on studio']);
	assert.deepEqual(groupChecks([]), { blocking: [], approvals: [], notes: [] });
});

test('requirements read as a sentence that names the Cua driver, never a coordinate action', () => {
	// The permissions are the driver's (#19), so needing them is needing a
	// driver on the destination; the capabilities are the desktop app's file
	// toolcalls when the task links a file.
	assert.equal(
		requiredSummary(card.required),
		'Needs a Cua driver with Screen Recording and Accessibility · file_read, file_list',
	);
	assert.equal(
		requiredSummary({ capabilities: [], permissions: ['screen_capture', 'accessibility'] }),
		'Needs a Cua driver with Screen Recording and Accessibility',
	);
	assert.equal(requiredSummary({ capabilities: [], permissions: [] }), '');
});

test('cards are kept newest first and dropped once they are no longer offered', () => {
	const older = { ...card, record_id: 'task_0', updated_at: NOW - 900 };
	let cards = upsertCard([older], card);
	assert.deepEqual(cards.map((c) => c.record_id), ['task_1', 'task_0']);
	cards = upsertCard(cards, { ...card, updated_at: NOW - 1000, goal: 'changed' });
	assert.deepEqual(cards.map((c) => [c.record_id, c.goal]), [['task_0', 'rename the trip photos'], ['task_1', 'changed']]);
	cards = upsertCard(cards, { ...card, offered: false });
	assert.deepEqual(cards.map((c) => c.record_id), ['task_0']);
	assert.deepEqual(upsertCard([], { ...card, offered: false }), []);
});
