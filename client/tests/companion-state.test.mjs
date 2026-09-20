import test from 'node:test';
import assert from 'node:assert/strict';
import {
	COMPANION_KINDS,
	STATE_EXAMPLES,
	companionEventFromServer,
	companionLabel,
	companionStatusText,
	initialCompanionState,
	permissionDenial,
	reduceCompanion,
} from '../src/lib/companion/state.js';

/** @param {object[]} events */
function run(events, state = initialCompanionState()) {
	return events.reduce((s, e) => reduceCompanion(s, e), state);
}

const online = { type: 'connection', connected: true, reconnecting: false, attempt: 0 };
const dropped = { type: 'connection', connected: false, reconnecting: true, attempt: 3 };
const running = { type: 'agent_running', chatId: 'chat-a' };
const stopped = { type: 'agent_stopped', chatId: 'chat-a' };
const reading = { type: 'action', chatId: 'chat-a', tool: 'read_file', summary: 'reading notes/tea.md' };
const remote = { type: 'action', chatId: 'chat-a', tool: 'computer_use', summary: 'opening Finder', machine: 'studio-mac' };

test('the companion is offline until the socket opens, then idle', () => {
	const initial = initialCompanionState();
	assert.equal(initial.kind, 'offline', 'nothing is assumed before the socket reports it is open');
	assert.equal(companionStatusText(initial), 'Nolune is connecting.');
	const idle = run([online]);
	assert.equal(idle.kind, 'idle');
	assert.equal(companionStatusText(idle), 'Nolune is idle.');
	assert.equal(companionStatusText(idle, 'Luna'), 'Luna is idle.', 'the companion name replaces the product name');
});

test('a received message is listening; the run starting is thinking', () => {
	const listening = run([online, { type: 'user_message', chatId: 'chat-a' }]);
	assert.equal(listening.kind, 'listening');
	assert.equal(companionStatusText(listening), 'Nolune is listening.');
	const thinking = run([running], listening);
	assert.equal(thinking.kind, 'thinking');
	assert.equal(companionStatusText(thinking), 'Nolune is thinking.');
	assert.equal(thinking.listening, false, 'the run picked the message up');
});

test('recalling shows before the first action and never overrides working', () => {
	const recalling = run([online, running, { type: 'memory_recall', chatId: 'chat-a', count: 3 }]);
	assert.equal(recalling.kind, 'recalling');
	assert.equal(companionStatusText(recalling), 'Nolune is recalling 3 memories.');
	assert.equal(companionStatusText(run([{ type: 'memory_recall', chatId: 'chat-a', count: 1 }], run([online, running]))), 'Nolune is recalling 1 memory.');
	const working = run([reading], recalling);
	assert.equal(working.kind, 'working', 'an action replaces the recall phase');
	const recalledAgain = run([{ type: 'memory_recall', chatId: 'chat-a', count: 2 }], working);
	assert.equal(recalledAgain.kind, 'working', 'a later recall does not hide the action in progress');
	assert.equal(recalledAgain.recalled, 5, 'recalls in the run still add up for the status detail');
});

test('working locally names the action and the computer it runs on', () => {
	const working = run([online, running, reading]);
	assert.equal(working.kind, 'working');
	assert.equal(companionStatusText(working), 'Nolune is working on this computer: reading notes/tea.md.');
	const thinkingAgain = run([{ type: 'assistant_message', chatId: 'chat-a' }], working);
	assert.equal(thinkingAgain.kind, 'thinking', 'a reply after the action means the action is over');
	const provesRun = run([online, reading]);
	assert.equal(provesRun.kind, 'working', 'a tool call proves the run is active even if agent_running was missed');
});

test('working on another computer names that computer in the accessible text', () => {
	const away = run([online, running, remote]);
	assert.equal(away.kind, 'working_remote');
	assert.equal(companionStatusText(away), 'Nolune is working on studio-mac: opening Finder.');
	assert.equal(companionLabel(away.kind), 'Working on another computer');
	const back = run([reading], away);
	assert.equal(back.kind, 'working', 'the next local action moves the work back here');
});

test('waiting for approval beats working and outlasts the run', () => {
	const ask = { type: 'approval_requested', id: 'req-1', prompt: 'a GitHub token for gh', target: 'GITHUB_TOKEN' };
	const waiting = run([online, running, reading, ask]);
	assert.equal(waiting.kind, 'waiting');
	assert.equal(companionStatusText(waiting), 'Nolune is waiting for you: a GitHub token for gh.');
	assert.equal(run([remote], waiting).kind, 'waiting', 'new actions do not hide the open request');
	const stoppedWhileWaiting = run([stopped], waiting);
	assert.equal(stoppedWhileWaiting.kind, 'waiting', 'a run ending with a request open is not completed');
	assert.equal(run([{ type: 'approval_resolved', id: 'other' }], waiting).kind, 'waiting', 'resolving another request changes nothing');
	assert.equal(run([{ type: 'approval_resolved', id: 'req-1' }], waiting).kind, 'working', 'the answer returns to the action in progress');
	assert.equal(run([{ type: 'approval_resolved', id: 'req-1' }], stoppedWhileWaiting).kind, 'idle', 'no completed after a request outlived the run');
});

test('blocked by permissions beats working and is never shown as completed', () => {
	const denied = { type: 'permission_denied', chatId: 'chat-a', reason: 'permission denied' };
	const blocked = run([online, running, reading, denied]);
	assert.equal(blocked.kind, 'blocked');
	assert.equal(companionStatusText(blocked), 'Nolune is blocked by permissions: reading notes/tea.md (permission denied).');
	assert.equal(run([remote], blocked).kind, 'blocked', 'later actions in the run stay behind the blocker');
	const ended = run([stopped], blocked);
	assert.equal(ended.kind, 'blocked', 'the run ending does not turn a blocker into success');
	assert.equal(ended.completed, false);
	assert.equal(run([{ type: 'user_message', chatId: 'chat-a' }], ended).kind, 'listening', 'the next message starts clean');
	assert.equal(run([running], ended).kind, 'thinking', 'a new run starts clean');
	const explicit = run([online, running, { type: 'permission_denied', chatId: 'chat-a', tool: 'run_command', summary: 'running command', reason: 'operation not permitted' }]);
	assert.equal(companionStatusText(explicit), 'Nolune is blocked by permissions: running command (operation not permitted).');
});

test('offline beats everything and keeps the work underneath', () => {
	const working = run([online, running, remote]);
	const offline = run([dropped], working);
	assert.equal(offline.kind, 'offline');
	assert.equal(companionStatusText(offline), 'Nolune is offline, reconnecting (attempt 3).');
	assert.equal(run([dropped], run([online, running, reading, { type: 'permission_denied', chatId: 'chat-a', reason: 'denied' }])).kind, 'offline');
	assert.equal(companionStatusText(run([{ type: 'connection', connected: false, reconnecting: false, attempt: 0 }], working)), 'Nolune is offline.');
	assert.equal(run([online], offline).kind, 'working_remote', 'reconnecting restores the state the runtime is still in');
});

test('completed only after agent_stopped without error, and only for a run that was seen', () => {
	assert.equal(run([online, stopped]).kind, 'idle', 'a stray stop at page load is not a success');
	const done = run([online, running, reading, stopped]);
	assert.equal(done.kind, 'completed');
	assert.equal(companionStatusText(done), 'Nolune finished.');
	assert.equal(done.action, null);
	assert.equal(run([{ type: 'settle' }], done).kind, 'idle', 'completed settles back to idle');
	const failed = run([online, running, { type: 'agent_stopped', chatId: 'chat-a', error: 'provider returned 500' }]);
	assert.equal(failed.kind, 'failed');
	assert.equal(companionStatusText(failed), 'Nolune stopped with an error: provider returned 500.');
	assert.equal(run([{ type: 'settle' }], failed).kind, 'failed', 'a failure does not settle into idle on its own');
	assert.equal(run([{ type: 'user_message', chatId: 'chat-a' }], failed).kind, 'listening');
	assert.equal(run([{ type: 'user_message', chatId: 'chat-a' }], done).kind, 'listening', 'a message after completion starts listening');
});

test('one companion, several chats: completed only when the last run stops', () => {
	const both = run([online, running, { type: 'agent_running', chatId: 'chat-b' }, reading]);
	assert.equal(both.kind, 'working');
	const oneLeft = run([stopped], both);
	assert.equal(oneLeft.kind, 'thinking', 'chat-a stopped, chat-b still runs; chat-a action is gone');
	assert.equal(oneLeft.action, null);
	assert.equal(run([{ type: 'agent_stopped', chatId: 'chat-b' }], oneLeft).kind, 'completed');
	const heard = run([{ type: 'user_message', chatId: 'chat-b' }], run([online, running, reading]));
	assert.equal(heard.kind, 'working', 'a message to a working companion is heard without interrupting the work');
});

test('the reducer is pure: no mutation, stable identity when nothing changes', () => {
	const before = run([online, running, reading]);
	const snapshot = JSON.stringify(before);
	const after = reduceCompanion(before, remote);
	assert.equal(JSON.stringify(before), snapshot, 'the previous state is not mutated');
	assert.notEqual(after, before);
	assert.equal(reduceCompanion(before, online), before, 'a repeated fact returns the same object');
	assert.equal(reduceCompanion(before, { type: 'approval_resolved', id: 'none' }), before);
	assert.equal(reduceCompanion(before, { type: 'settle' }), before);
	assert.deepEqual(run([online, running, reading, remote]), run([online, running, reading, remote]), 'same events, same state');
	assert.ok(Object.isFrozen(initialCompanionState()));
});

test('server events map onto the reducer vocabulary without inventing state', () => {
	const slug = 'companion';
	assert.deepEqual(
		companionEventFromServer({ type: 'chat_message_created', instance_slug: slug, chat_id: 'c', message: { id: 'm1', role: 'user', content: 'hi', created_at: '1' } }),
		{ type: 'user_message', chatId: 'c' },
	);
	assert.deepEqual(
		companionEventFromServer({ type: 'chat_message_created', instance_slug: slug, chat_id: 'c', message: { id: 'm2', role: 'assistant', content: 'hello', created_at: '1', kind: 'message' } }),
		{ type: 'assistant_message', chatId: 'c' },
	);
	assert.deepEqual(
		companionEventFromServer({ type: 'chat_message_created', instance_slug: slug, chat_id: 'c', message: { id: 't1', role: 'assistant', content: 'reading notes/tea.md', created_at: '1', kind: 'tool_call', tool_name: 'read_file' } }),
		{ type: 'action', chatId: 'c', tool: 'read_file', summary: 'reading notes/tea.md', machine: null },
	);
	assert.deepEqual(
		companionEventFromServer({ type: 'chat_message_created', instance_slug: slug, chat_id: 'c', message: { id: 't2', role: 'assistant', content: 'error: bash: /etc/hosts: Permission denied', created_at: '1', kind: 'tool_output', tool_name: 'run_command' } }),
		{ type: 'permission_denied', chatId: 'c', tool: 'run_command', reason: 'bash: /etc/hosts: Permission denied' },
	);
	assert.equal(
		companionEventFromServer({ type: 'chat_message_created', instance_slug: slug, chat_id: 'c', message: { id: 't3', role: 'assistant', content: 'total 4\ndrwxr-xr-x', created_at: '1', kind: 'tool_output', tool_name: 'run_command' } }),
		null,
		'ordinary tool output is not a state change',
	);
	assert.deepEqual(companionEventFromServer({ type: 'tool_activity', instance_slug: slug, chat_id: 'c', tool_name: 'web_search', summary: 'web search: tea' }), { type: 'action', chatId: 'c', tool: 'web_search', summary: 'web search: tea', machine: null });
	assert.deepEqual(companionEventFromServer({ type: 'agent_running', instance_slug: slug, chat_id: 'c' }), { type: 'agent_running', chatId: 'c' });
	assert.deepEqual(companionEventFromServer({ type: 'agent_stopped', instance_slug: slug, chat_id: 'c' }), { type: 'agent_stopped', chatId: 'c' });
	assert.deepEqual(companionEventFromServer({ type: 'memory_recall', instance_slug: slug, chat_id: 'c', memories: [{ path: 'a', preview: '', score: 1 }, { path: 'b', preview: '', score: 1 }] }), { type: 'memory_recall', chatId: 'c', count: 2 });
	assert.deepEqual(
		companionEventFromServer({ type: 'secret_request', instance_slug: slug, id: 'req-1', prompt: 'a GitHub token for gh', target: 'GITHUB_TOKEN' }),
		{ type: 'approval_requested', id: 'req-1', prompt: 'a GitHub token for gh', target: 'GITHUB_TOKEN' },
	);
	assert.equal(companionEventFromServer({ type: 'mood_updated', instance_slug: slug, mood: 'calm' }), null);
	assert.equal(companionEventFromServer({ type: 'chat_stream_delta', instance_slug: slug, chat_id: 'c', message_id: 'm', delta: 'x' }), null);
});

test('permission denials are recognised from tool output, everything else is not a blocker', () => {
	assert.equal(permissionDenial('error: bash: /etc/hosts: Permission denied'), 'bash: /etc/hosts: Permission denied');
	assert.equal(permissionDenial('error: rm: /System: Operation not permitted'), 'rm: /System: Operation not permitted');
	assert.equal(permissionDenial('error: reach_out is not allowed right now: quiet hours'), 'reach_out is not allowed right now: quiet hours');
	assert.equal(permissionDenial('error: EACCES: permission denied, open /var/log'), 'EACCES: permission denied, open /var/log');
	assert.equal(permissionDenial('error: command not found'), null);
	assert.equal(permissionDenial('permission denied'), null, 'only a tool error line counts, not text that merely mentions permissions');
	assert.equal(permissionDenial(''), null);
});

test('the design-system gallery is produced by the reducer and covers every state once', () => {
	const kinds = COMPANION_KINDS.map((k) => k.kind);
	assert.deepEqual(kinds, ['offline', 'blocked', 'waiting', 'failed', 'working_remote', 'working', 'recalling', 'thinking', 'listening', 'completed', 'idle'], 'listed in priority order');
	assert.deepEqual(STATE_EXAMPLES.map((e) => e.kind).sort(), [...kinds].sort(), 'one example per state');
	for (const example of STATE_EXAMPLES) {
		const state = run(example.events);
		assert.equal(state.kind, example.kind, `${example.kind} example reduces to its own state`);
		const text = companionStatusText(state, 'Luna');
		assert.match(text, /^Luna .+\.$/, `${example.kind} status is a sentence about the companion: ${text}`);
		assert.ok(companionLabel(example.kind).length > 0);
	}
	const away = STATE_EXAMPLES.find((e) => e.kind === 'working_remote');
	assert.match(companionStatusText(run(away.events)), /studio-mac/, 'the remote example names its machine');
	assert.ok(Object.isFrozen(STATE_EXAMPLES) && Object.isFrozen(COMPANION_KINDS));
});
