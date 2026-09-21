import test from 'node:test';
import assert from 'node:assert/strict';
import {
	COMPANION_KINDS,
	STATE_EXAMPLES,
	companionEventFromServer,
	companionLabel,
	companionStatusSegments,
	companionStatusText,
	initialCompanionState,
	machineFromTrail,
	permissionDenial,
	reduceCompanion,
} from '../src/lib/companion/state.js';

/** @param {object[]} events */
function run(events, state = initialCompanionState()) {
	return events.reduce((s, e) => reduceCompanion(s, e), state);
}

const slug = 'companion';
/** A `chat_message_created` websocket event as the server sends it. */
function serverMessage(id, role, content, extra = {}) {
	return { type: 'chat_message_created', instance_slug: slug, chat_id: 'c', message: { id, role, content, created_at: '1', ...extra } };
}
/** Replays raw server events through the adapter, dropping the ones that say nothing. */
function replay(serverEvents, state = run([online])) {
	return run(serverEvents.map(companionEventFromServer).filter((e) => e !== null), state);
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
	const replied = run([{ type: 'assistant_message', chatId: 'chat-a' }], recalledAgain);
	assert.equal(replied.kind, 'thinking', 'a reply after an action is thinking, not a stale recall phase');
	assert.equal(companionStatusText(replied), 'Nolune is thinking.');
	const secondCycle = run([reading, { type: 'assistant_message', chatId: 'chat-a' }], replied);
	assert.equal(secondCycle.kind, 'thinking', 'nor after a second tool cycle');
	assert.equal(run([{ type: 'memory_recall', chatId: 'chat-a', count: 1 }], secondCycle).kind, 'thinking', 'a recall after the first action only adds to the count');
	const nextRun = run([stopped, running, { type: 'memory_recall', chatId: 'chat-a', count: 2 }], secondCycle);
	assert.equal(nextRun.kind, 'recalling', 'the next run starts its own recall phase');
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

test('a run the server reports as failed is never shown as completed', () => {
	// The exact production sequence: there is no error field on agent_stopped;
	// a failed turn is a `[system] <label>` assistant message followed by the stop.
	const failedRun = [
		serverMessage('u1', 'user', 'hi'),
		{ type: 'agent_running', instance_slug: slug, chat_id: 'c' },
		serverMessage('t1', 'assistant', 'running command', { kind: 'tool_call', tool_name: 'run_command' }),
		serverMessage('s1', 'assistant', '[system] something went wrong', { kind: 'message' }),
		{ type: 'agent_stopped', instance_slug: slug, chat_id: 'c' },
	];
	const failed = replay(failedRun);
	assert.equal(failed.kind, 'failed');
	assert.equal(failed.completed, false);
	assert.equal(failed.action, null, 'the failed turn ended the action');
	assert.equal(companionStatusText(failed), 'Nolune stopped with an error: something went wrong.');
	assert.equal(run([{ type: 'settle' }], failed).kind, 'failed', 'a failure never settles into idle on its own');
	assert.equal(run([{ type: 'user_message', chatId: 'c' }], failed).kind, 'listening', 'the next message starts clean');
	const noKey = replay([
		serverMessage('u1', 'user', 'hi'),
		{ type: 'agent_running', instance_slug: slug, chat_id: 'c' },
		serverMessage('s1', 'assistant', '[system] no API key configured — add one in Settings'),
		{ type: 'agent_stopped', instance_slug: slug, chat_id: 'c' },
	]);
	assert.equal(noKey.kind, 'failed');
	assert.equal(companionStatusText(noKey), 'Nolune stopped with an error: no API key configured — add one in Settings.');
	assert.equal(replay(failedRun.slice(0, 4)).kind, 'failed', 'the failure shows as soon as it is reported, before the stop arrives');
	// Status lines are also `[system]` assistant messages but say nothing about the run.
	const working = replay(failedRun.slice(0, 3));
	assert.equal(working.kind, 'working');
	assert.equal(replay([serverMessage('s2', 'assistant', '[system] mood → curious')], working), working, 'a mood line is not a reply and not a failure');
	assert.equal(replay([serverMessage('s2', 'assistant', '[system] mood → curious'), { type: 'agent_stopped', instance_slug: slug, chat_id: 'c' }], working).kind, 'completed');
	assert.deepEqual(companionEventFromServer(serverMessage('s1', 'assistant', '[system] something went wrong', { kind: 'message' })), { type: 'run_failed', chatId: 'c', error: 'something went wrong' });
	assert.deepEqual(companionEventFromServer(serverMessage('s1', 'assistant', '[system] request timed out')), { type: 'run_failed', chatId: 'c', error: 'request timed out' });
	for (const line of ['[system] mood → calm', '[system] rhythm update\nmornings are busy', "[system] routine 'check-in' ran (120 tokens)", "[system] desktop 'studio-mac' connected.", "[system] user left this instance. desktop 'studio-mac' still connected to server."]) {
		assert.equal(companionEventFromServer(serverMessage('s', 'assistant', line)), null, `${line} is a status line, not a failure`);
	}
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
	// The tool itself failed (spawn error, policy refusal): `error: <reason>`.
	assert.equal(permissionDenial('error: bash: /etc/hosts: Permission denied'), 'bash: /etc/hosts: Permission denied');
	assert.equal(permissionDenial('error: rm: /System: Operation not permitted'), 'rm: /System: Operation not permitted');
	assert.equal(permissionDenial('error: reach_out is not allowed right now: quiet hours'), 'reach_out is not allowed right now: quiet hours');
	assert.equal(permissionDenial('error: EACCES: permission denied, open /var/log'), 'EACCES: permission denied, open /var/log');
	assert.equal(permissionDenial('error: failed to execute command: Permission denied (os error 13)'), 'failed to execute command: Permission denied (os error 13)');
	assert.equal(permissionDenial('error: command not found'), null);
	// run_command without a PTY returns a non-zero exit as Ok: stdout, then `stderr: …`.
	assert.equal(permissionDenial('stderr: ls: /root: Permission denied'), 'ls: /root: Permission denied');
	assert.equal(permissionDenial('total 4\ndrwxr-xr-x  notes\nstderr: cat: /etc/shadow: Permission denied'), 'cat: /etc/shadow: Permission denied');
	assert.equal(permissionDenial('stderr: warning: unused variable\nrm: /System: Operation not permitted\n'), 'rm: /System: Operation not permitted', 'the denial may be on a later stderr line');
	assert.equal(permissionDenial('stderr: warning: unused variable'), null, 'stderr without a denial is not a blocker');
	// run_command with a PTY (the default) returns the raw terminal output: no marker at all.
	assert.equal(permissionDenial('ls: /root: Permission denied\r\n'), 'ls: /root: Permission denied');
	assert.equal(permissionDenial('zsh: permission denied: ./deploy.sh'), 'zsh: permission denied: ./deploy.sh');
	assert.equal(permissionDenial("mkdir: cannot create directory '/srv/x': Permission denied"), "mkdir: cannot create directory '/srv/x': Permission denied");
	assert.equal(permissionDenial('git@github.com: Permission denied (publickey).\r\nfatal: Could not read from remote repository.'), 'git@github.com: Permission denied (publickey).');
	assert.equal(permissionDenial("Error: EACCES: permission denied, open '/var/log/app.log'"), "Error: EACCES: permission denied, open '/var/log/app.log'");
	assert.equal(permissionDenial("PermissionError: [Errno 13] Permission denied: '/etc/shadow'"), "PermissionError: [Errno 13] Permission denied: '/etc/shadow'");
	assert.equal(permissionDenial('docker: permission denied while trying to connect to the Docker daemon socket'), 'docker: permission denied while trying to connect to the Docker daemon socket');
	// Ordinary output that merely mentions permissions is not a blocker.
	assert.equal(permissionDenial('permission denied'), null, 'only a diagnostic line counts, not text that merely mentions permissions');
	assert.equal(permissionDenial('Note: if you see permission denied, run chmod +x first'), null);
	assert.equal(permissionDenial('# Troubleshooting\nEACCES errors mean the socket is owned by root.'), null, 'prose about error codes is not a diagnostic');
	assert.equal(permissionDenial('total 4\ndrwxr-xr-x'), null);
	assert.equal(permissionDenial('command completed with exit code 1'), null);
	assert.equal(permissionDenial(''), null);
});

test('a real run_command denial reduces to blocked and outlives the stop', () => {
	const denied = replay([
		{ type: 'agent_running', instance_slug: slug, chat_id: 'c' },
		serverMessage('t1', 'assistant', 'running command', { kind: 'tool_call', tool_name: 'run_command' }),
		serverMessage('t2', 'assistant', 'stderr: ls: /root: Permission denied', { kind: 'tool_output', tool_name: 'run_command' }),
	]);
	assert.equal(denied.kind, 'blocked');
	assert.equal(companionStatusText(denied), 'Nolune is blocked by permissions: running command (ls: /root: Permission denied).');
	const ended = replay([{ type: 'agent_stopped', instance_slug: slug, chat_id: 'c' }], denied);
	assert.equal(ended.kind, 'blocked');
	assert.equal(ended.completed, false);
	const pty = replay([
		{ type: 'agent_running', instance_slug: slug, chat_id: 'c' },
		serverMessage('t1', 'assistant', 'running command', { kind: 'tool_call', tool_name: 'run_command' }),
		serverMessage('t2', 'assistant', 'zsh: permission denied: ./deploy.sh\r\n', { kind: 'tool_output', tool_name: 'run_command' }),
	]);
	assert.equal(pty.kind, 'blocked', 'the PTY path (the default) has no stderr marker');
	assert.equal(companionStatusText(pty), 'Nolune is blocked by permissions: running command (zsh: permission denied: ./deploy.sh).');
	assert.deepEqual(
		companionEventFromServer(serverMessage('t2', 'assistant', 'stderr: ls: /root: Permission denied', { kind: 'tool_output', tool_name: 'run_command' })),
		{ type: 'permission_denied', chatId: 'c', tool: 'run_command', reason: 'ls: /root: Permission denied' },
	);
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

/** The text a list of status segments reads as. @param {readonly { text: string; href?: string }[]} segments */
const joined = (segments) => segments.map((s) => s.text).join('');

test('a desktop tool call names the computer from its trail line', () => {
	// #80 persisted the trail line the tool announced: "<action> on <computer>",
	// by the name the Computers tab shows (else the id).
	assert.deepEqual(machineFromTrail('computer_use', 'screenshot on Studio Mac'), { summary: 'screenshot', machine: 'Studio Mac' });
	assert.deepEqual(machineFromTrail('remote_bash', 'running a command on studio-mac'), { summary: 'running a command', machine: 'studio-mac' });
	assert.deepEqual(machineFromTrail('remote_files', 'reading ~/notes on tea.md on Studio Mac'), { summary: 'reading ~/notes on tea.md', machine: 'Studio Mac' }, 'the computer is what follows the last " on "');
	assert.deepEqual(machineFromTrail('computer_use', 'screenshot on the server home'), { summary: 'screenshot', machine: null }, 'the server home is this computer');
	assert.deepEqual(machineFromTrail('computer_use', 'screenshot on the connected computer'), { summary: 'screenshot', machine: 'the connected computer' }, 'a call with the choice still open names no computer, and says so');
	assert.deepEqual(machineFromTrail('read_file', 'reading notes on tea.md'), { summary: 'reading notes on tea.md', machine: null }, 'only the desktop tools act on another computer');
	assert.deepEqual(machineFromTrail('computer_use', 'screenshot'), { summary: 'screenshot', machine: null }, 'a trail line recorded before #80 names nothing');
	// Through the adapter, from the events the server actually sends.
	assert.deepEqual(
		companionEventFromServer(serverMessage('t1', 'assistant', 'left_click on Studio Mac', { kind: 'tool_call', tool_name: 'computer_use' })),
		{ type: 'action', chatId: 'c', tool: 'computer_use', summary: 'left_click', machine: 'Studio Mac' },
	);
	assert.deepEqual(
		companionEventFromServer({ type: 'tool_activity', instance_slug: slug, chat_id: 'c', tool_name: 'remote_files', summary: 'writing ~/notes.md on Studio Mac' }),
		{ type: 'action', chatId: 'c', tool: 'remote_files', summary: 'writing ~/notes.md', machine: 'Studio Mac' },
	);
	const away = replay([
		{ type: 'agent_running', instance_slug: slug, chat_id: 'c' },
		serverMessage('t1', 'assistant', 'screenshot on Studio Mac', { kind: 'tool_call', tool_name: 'computer_use' }),
	]);
	assert.equal(away.kind, 'working_remote');
	assert.equal(companionStatusText(away), 'Nolune is working on Studio Mac: screenshot.');
	const home = replay([serverMessage('t2', 'assistant', 'screenshot on the server home', { kind: 'tool_call', tool_name: 'computer_use' })], away);
	assert.equal(home.kind, 'working', 'the server home is where the companion lives');
	assert.equal(companionStatusText(home), 'Nolune is working on this computer: screenshot.');
});

test('a proactive run is a run the companion is on, and its outcome is never invented', () => {
	const machines = [{ machine_id: 'm-1', display_name: 'Studio Mac' }];
	const receipt = (status, over = {}) => ({ type: 'activity_updated', instance_slug: slug, run: { version: 1, id: 'r1', trigger: { kind: 'heartbeat', agent: 'companion' }, reason: 'periodic', target: { kind: 'companion' }, dedupe_key: 'heartbeat:companion', status, attempt: 1, started_at: 1, approvals: [], ...over } });
	assert.deepEqual(companionEventFromServer(receipt({ kind: 'running' })), { type: 'activity_run', id: 'r1', status: 'running', label: 'Check-in', machine: null, handoffId: null, error: null });
	assert.deepEqual(
		companionEventFromServer(receipt({ kind: 'running' }, { trigger: { kind: 'handoff', handoff_id: 'h1' }, target: { kind: 'machine', machine_id: 'm-1' } }), machines),
		{ type: 'activity_run', id: 'r1', status: 'running', label: 'Handoff on Studio Mac', machine: 'Studio Mac', handoffId: 'h1', error: null },
		'a run targeting a computer names it the way the Computers tab does',
	);
	assert.deepEqual(companionEventFromServer(receipt({ kind: 'failed', error: 'no model configured', retryable: true })), { type: 'activity_run', id: 'r1', status: 'failed', label: 'Check-in', machine: null, handoffId: null, error: 'no model configured' });
	// Running: the companion is on it, with nothing more claimed than the run itself.
	const thinking = replay([receipt({ kind: 'running' })]);
	assert.equal(thinking.kind, 'thinking');
	assert.equal(companionStatusText(thinking), 'Nolune is thinking: Check-in.');
	assert.equal(replay([{ type: 'memory_recall', instance_slug: slug, chat_id: 'default', memories: [{ path: 'a', preview: '', score: 1 }] }], thinking).kind, 'recalling');
	// Completed: only because the server said so.
	const done = replay([receipt({ kind: 'completed', finished_at: 2 })], thinking);
	assert.equal(done.kind, 'completed');
	assert.equal(companionStatusText(done), 'Nolune finished: Check-in.');
	assert.equal(run([{ type: 'settle' }], done).kind, 'idle');
	// Failed: the error, never completed.
	const failed = replay([receipt({ kind: 'failed', error: 'no model configured', retryable: true })], thinking);
	assert.equal(failed.kind, 'failed');
	assert.equal(failed.completed, false);
	assert.equal(companionStatusText(failed), 'Nolune stopped with an error: no model configured.');
	assert.equal(replay([receipt({ kind: 'failed', error: 'no model configured', retryable: true })]).kind, 'failed', 'a failure is reported even for a run that was not seen starting');
	// Cancelled: over, with no success claimed.
	const cancelled = replay([receipt({ kind: 'cancelled' })], thinking);
	assert.equal(cancelled.kind, 'idle');
	assert.equal(cancelled.completed, false);
	// Skipped or finished runs the client never saw start say nothing.
	const idle = run([online]);
	assert.equal(replay([receipt({ kind: 'skipped', reason: { kind: 'quiet_hours' } })], idle), idle);
	assert.equal(replay([receipt({ kind: 'completed' })], idle), idle, 'a stray completion is not a success');
	// A chat run and a proactive run at once: the companion is busy until the last one stops.
	const both = replay([receipt({ kind: 'running' })], run([online, running, reading]));
	assert.equal(both.kind, 'working', 'the action in progress is what shows');
	const chatDone = run([stopped], both);
	assert.equal(chatDone.kind, 'thinking');
	assert.equal(companionStatusText(chatDone), 'Nolune is thinking: Check-in.');
	assert.equal(replay([receipt({ kind: 'completed' })], chatDone).kind, 'completed');
	// The next message or run starts clean.
	assert.equal(companionStatusText(run([{ type: 'user_message', chatId: 'chat-a' }], done)), 'Nolune is listening.');
	assert.equal(companionStatusText(run([running], done)), 'Nolune is thinking.');
});

test('a conversation snapshot is persisted state: it starts a missed run and ends one without claiming success', () => {
	// GET /chat says agent_running; the client may have missed agent_running (page load, reconnect).
	const missed = run([online, { type: 'snapshot', chatId: 'chat-a', running: true }]);
	assert.equal(missed.kind, 'thinking');
	assert.equal(run([{ type: 'snapshot', chatId: 'chat-a', running: true }], missed), missed, 'a snapshot of a run already seen changes nothing');
	// The run ended while the client was away: over, but nobody said it succeeded.
	const working = run([online, running, reading]);
	const ended = run([{ type: 'snapshot', chatId: 'chat-a', running: false }], working);
	assert.equal(ended.kind, 'idle');
	assert.equal(ended.completed, false);
	assert.equal(ended.action, null);
	const idle = run([online]);
	assert.equal(run([{ type: 'snapshot', chatId: 'chat-a', running: false }], idle), idle, 'a snapshot of nothing running changes nothing');
	const held = run([online, running, reading, stopped]);
	assert.equal(run([{ type: 'snapshot', chatId: 'chat-a', running: false }], held), held, 'nor does it cut a completed hold short');
	const blocked = run([online, running, reading, { type: 'permission_denied', chatId: 'chat-a', reason: 'permission denied' }]);
	assert.equal(run([{ type: 'snapshot', chatId: 'chat-a', running: false }], blocked).kind, 'blocked', 'the blocker outlives the run either way');
});

test('the status text links to the machine, the run, the handoff or the blocker', () => {
	const at = 'companion';
	const remoteWork = run([online, running, remote]);
	assert.deepEqual(companionStatusSegments(remoteWork, 'Nolune', at), [
		{ text: 'Nolune is working on ' },
		{ text: 'studio-mac', href: '/companion/computers' },
		{ text: ': opening Finder.' },
	]);
	assert.equal(joined(companionStatusSegments(remoteWork, 'Nolune', at)), companionStatusText(remoteWork), 'the segments read as the status sentence');
	assert.deepEqual(companionStatusSegments(run([online, running, reading]), 'Luna', at), [
		{ text: 'Luna is working on this computer: ' },
		{ text: 'reading notes/tea.md', href: '/companion/chat/chat-a' },
		{ text: '.' },
	], 'an action links to its conversation');
	const main = run([online, { type: 'action', chatId: 'default', tool: 'read_file', summary: 'reading notes/tea.md' }]);
	assert.equal(companionStatusSegments(main, 'Luna', at)[1].href, '/companion/chat', 'the default conversation is the chat tab itself');
	const blocked = run([online, running, { type: 'action', chatId: 'chat-a', tool: 'run_command', summary: 'running command' }, { type: 'permission_denied', chatId: 'chat-a', reason: 'ls: /root: Permission denied' }]);
	assert.deepEqual(companionStatusSegments(blocked, 'Nolune', at), [
		{ text: 'Nolune is blocked by permissions: ' },
		{ text: 'running command', href: '/companion/chat/chat-a' },
		{ text: ' (ls: /root: Permission denied).' },
	]);
	const activity = { type: 'activity_run', id: 'r1', status: 'running', label: 'Check-in', machine: null, handoffId: null, error: null };
	const onRun = run([online, activity]);
	assert.deepEqual(companionStatusSegments(onRun, 'Nolune', at), [
		{ text: 'Nolune is thinking: ' },
		{ text: 'Check-in', href: '/companion/activity#run-r1' },
		{ text: '.' },
	]);
	assert.equal(companionStatusSegments(run([{ ...activity, status: 'completed' }], onRun), 'Nolune', at)[1].href, '/companion/activity#run-r1', 'the completed run stays linked');
	assert.deepEqual(companionStatusSegments(run([{ ...activity, status: 'failed', error: 'no model configured' }], onRun), 'Nolune', at), [
		{ text: 'Nolune stopped with an error: ' },
		{ text: 'no model configured', href: '/companion/activity#run-r1' },
		{ text: '.' },
	]);
	const handoff = run([online, { ...activity, id: 'r2', label: 'Handoff on Studio Mac', machine: 'Studio Mac', handoffId: 'h1' }]);
	assert.equal(companionStatusSegments(handoff, 'Nolune', at)[1].href, '/companion/activity#handoff-card-h1', 'a continuation links to its handoff card');
	const chatFailure = run([online, running, { type: 'run_failed', chatId: 'chat-a', error: 'something went wrong' }]);
	assert.equal(companionStatusSegments(chatFailure, 'Nolune', at)[1].href, '/companion/chat/chat-a', 'a failed chat turn links to the conversation');
	for (const state of [run([online]), run([online, { type: 'user_message', chatId: 'chat-a' }]), run([online, running]), run([dropped]), run([online, running, { type: 'approval_requested', id: 'q', prompt: 'a GitHub token for gh' }])]) {
		const segments = companionStatusSegments(state, 'Nolune', at);
		assert.equal(segments.length, 1, `${state.kind} has nothing to link to`);
		assert.equal(segments[0].href, undefined);
		assert.equal(segments[0].text, companionStatusText(state));
	}
	assert.deepEqual(companionStatusSegments(remoteWork), [{ text: 'Nolune is working on studio-mac: opening Finder.' }], 'without a companion route there is nothing to link, and the sentence is unchanged');
	assert.ok(Object.isFrozen(companionStatusSegments(remoteWork, 'Nolune', at)));
});
