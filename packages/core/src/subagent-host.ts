import { hasBackgroundCommands, stopBackgroundCommands } from './background.ts';
import type { DisplayMessage } from './conversations.ts';
import {
	isRunning,
	kick,
	onCommandEnd,
	onLoopEnd,
	refreshBackground,
	stop,
	stoppedBy,
	subscribe
} from './runner.ts';
import {
	activeSubagents,
	appendSubagentLog,
	isActive,
	setSubagentStatus,
	settleSubagent,
	subagentByConversation,
	subagentsWithStatus,
	type Subagent
} from './subagents.ts';

/**
 * Gateway only: runs the subagents `nolune agent run` asks for (subagents.ts). A pending subagent has
 * messages it hasn't started on; the gateway starts its conversation through the normal runner,
 * writes its log as it works, and when its loop ends records how, which `nolune agent watch` waits for.
 */

const holder = globalThis as unknown as { __noluneSubagentHost?: { logging: Set<string> } };

/** Starts pending subagents and stops the ones `nolune agent stop` asked to. Called every tick. */
export function processSubagents(): void {
	for (const s of subagentsWithStatus('pending')) {
		setSubagentStatus(s.id, 'running');
		writeLog(s);
		// A running loop picks up the new message at its next step.
		kick(s.conversationId);
		refreshBackground(s.parentId);
	}
	for (const s of subagentsWithStatus('stopping')) {
		stopBackgroundCommands(s.conversationId, 'nolune');
		// A running one is marked stopped when its loop ends.
		if (isRunning(s.conversationId)) stop(s.conversationId, 'nolune');
		else markStopped(s);
	}
}

function markStopped(s: Subagent): void {
	const why = s.error ?? 'Stopped.';
	setSubagentStatus(s.id, 'stopped', why);
	appendSubagentLog(s, `\n=== ${clock()} · stopped. ${why}\n`);
	refreshBackground(s.parentId);
}

/** When a subagent's loop ends, its work is over unless something is still coming its way. */
export function finishSubagent(conversationId: string, error: string | null): void {
	const s = subagentByConversation(conversationId);
	if (!s) return;
	if (s.status === 'stopping') {
		markStopped(s);
		return;
	}
	if (s.status !== 'running') return; // pending: started again below or at the next tick
	const who = stoppedBy(conversationId);
	// Still working while its background commands run: their output starts it again.
	if (!error && !who && hasBackgroundCommands(conversationId)) return;
	const [status, why] = error
		? (['failed', error] as const)
		: who
			? (['stopped', `Stopped by ${who}.`] as const)
			: (['done', null] as const);
	const settled = settleSubagent(conversationId, status, why);
	if (settled === 'again') {
		// A steer or more work arrived just as it finished.
		processSubagents();
		return;
	}
	if (settled === 'skipped') return;
	appendSubagentLog(
		s,
		status === 'done'
			? `\n=== ${clock()} · done. Its last message above is its result.\n`
			: `\n=== ${clock()} · ${status}: ${why}\n`
	);
	refreshBackground(s.parentId);
}

/**
 * Stop in a chat: the reply, the commands running in its background and its subagents. Nothing
 * they would have handed over reaches the conversation.
 */
export function stopConversation(conversationId: string, byName: string): void {
	stop(conversationId, byName);
	stopBackgroundCommands(conversationId, byName);
	// Its subagents, and, for a subagent's own chat, the subagent: stopped in processSubagents, also
	// when it was only waiting for background commands and no loop is left to end.
	const own = subagentByConversation(conversationId);
	for (const s of [...activeSubagents(conversationId), ...(own && isActive(own) ? [own] : [])]) {
		setSubagentStatus(s.id, 'stopping', `Stopped by ${byName}.`);
		stop(s.conversationId, byName);
	}
	processSubagents();
	refreshBackground(conversationId);
}

/** After a restart: subagents that were working start again (their commands were cut off). */
export function recoverSubagents(): void {
	for (const s of subagentsWithStatus('running')) setSubagentStatus(s.id, 'pending');
}

/** Gateway only, from startScheduler. */
export function startSubagentHost(): void {
	onLoopEnd(finishSubagent);
	onCommandEnd(processSubagents);
	recoverSubagents();
}

// --- the log ---

function clock(date = new Date()): string {
	return date.toLocaleTimeString('en-GB', { hour12: false });
}

const OUTPUT_CHARS = 2_000;

function clip(text: string): string {
	return text.length > OUTPUT_CHARS
		? `${text.slice(0, OUTPUT_CHARS)}\n[... ${text.length - OUTPUT_CHARS} more characters]`
		: text;
}

function indent(text: string): string {
	return text.replace(/^/gm, '    ');
}

/** One committed row as the log shows it: what was said and run, never the reasoning. */
export function logEntry(name: string, message: DisplayMessage): string {
	const at = clock(new Date(message.createdAt));
	switch (message.kind) {
		case 'agent_message':
			return `\n=== ${at} · from the agent that started ${name}\n${message.text}\n`;
		case 'assistant': {
			const parts = message.blocks.flatMap((b) => {
				if (b.type === 'text') return [b.text.trim()];
				if (b.type === 'tool') {
					return [`$ ${b.command}${b.summary ? `\n  (${b.summary})` : ''}`];
				}
				return [];
			});
			return parts.length ? `\n=== ${at} · ${name}\n${parts.join('\n\n')}\n` : '';
		}
		case 'tool_results':
			return message.results.map((r) => `${indent(clip(r.output.trim()))}\n`).join('');
		case 'task_result':
			return `\n=== ${at} · background command ended: ${message.title}\n${indent(clip(message.output.trim()))}\n`;
		default:
			return '';
	}
}

/** Appends every row the subagent's conversation commits to its log, once per gateway start. */
function writeLog(s: Subagent): void {
	const state = (holder.__noluneSubagentHost ??= { logging: new Set() });
	if (state.logging.has(s.conversationId)) return;
	state.logging.add(s.conversationId);
	subscribe(s.conversationId, (event) => {
		if (event.type !== 'message') return;
		const entry = logEntry(s.name, event.message);
		if (entry) appendSubagentLog(s, entry);
	});
}
