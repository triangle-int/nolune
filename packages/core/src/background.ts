import { asc, eq, isNotNull } from 'drizzle-orm';
import { getConversation } from './conversations.ts';
import { getDb } from './db/index.ts';
import { backgroundCommand } from './db/schema.ts';
import { runCommand, type RunCommandInput, type RunCommandResult } from './run-command.ts';
import {
	SubagentError,
	activeSubagents,
	findSubagent,
	requestSubagentStop,
	subagentLogPath,
	type Subagent
} from './subagents.ts';

/**
 * Commands the agent started with `run_in_background`. The runner answers the call at once and,
 * when the command ends, hands its output to the conversation as a queued message (runner.ts).
 * The processes live in the gateway; a row per command lets `nolune background` list and stop them
 * from any process, and the next start tell the conversation that a restart cut it off.
 */

export interface BackgroundCommand {
	/** The run_command call that started it. */
	id: string;
	conversationId: string;
	summary: string | null;
	command: string;
	/** The shell's pid, which is also its process group. */
	pid: number;
	startedAt: number;
	abort: AbortController;
	/** Set when someone stopped it: nobody is told when it ends. */
	stoppedBy: string | null;
}

const holder = globalThis as unknown as { __noluneBackground?: Map<string, BackgroundCommand> };
const running = (holder.__noluneBackground ??= new Map());

export function backgroundCommands(conversationId: string): BackgroundCommand[] {
	return [...running.values()].filter((c) => c.conversationId === conversationId);
}

/**
 * Whether the conversation still has background commands that will hand their output over: one
 * being stopped won't, so nothing waits for it.
 */
export function hasBackgroundCommands(conversationId: string): boolean {
	return backgroundCommands(conversationId).some((c) => !c.stoppedBy);
}

/**
 * Starts the command. Resolves with it once the shell is running, or with the result if it
 * couldn't start (a missing folder, say), which then goes back to the model as usual. `onEnd`
 * gets the result when a started command ends, and whether someone stopped it.
 */
export function startBackgroundCommand(opts: {
	conversationId: string;
	toolUseId: string;
	summary: string | null;
	input: RunCommandInput;
	defaultCwd: string;
	env: NodeJS.ProcessEnv;
	onEnd: (command: BackgroundCommand, result: RunCommandResult) => void;
}): Promise<{ started: BackgroundCommand } | { result: RunCommandResult }> {
	const abort = new AbortController();
	let started: BackgroundCommand | null = null;
	const done = runCommand(opts.input, {
		defaultCwd: opts.defaultCwd,
		env: opts.env,
		signal: abort.signal,
		abortReason: () => `Stopped by ${started?.stoppedBy ?? 'a user'}.`,
		onStart: (pid) => {
			started = {
				id: opts.toolUseId,
				conversationId: opts.conversationId,
				summary: opts.summary,
				command: opts.input.command,
				pid,
				startedAt: Date.now(),
				abort,
				stoppedBy: null
			};
			running.set(opts.toolUseId, started);
			getDb()
				.insert(backgroundCommand)
				.values({
					toolUseId: opts.toolUseId,
					conversationId: opts.conversationId,
					summary: opts.summary,
					command: opts.input.command,
					pid
				})
				.onConflictDoNothing()
				.run();
		}
	});
	// runCommand spawns synchronously, so onStart has run by now if the shell started at all.
	const command = started as BackgroundCommand | null;
	if (!command) return done.then((result) => ({ result }));
	done
		.catch((err: unknown): RunCommandResult => ({
			content: `Not finished: ${err instanceof Error ? err.message : String(err)}`,
			isError: true,
			exitCode: null
		}))
		.then((result) => {
			running.delete(command.id);
			getDb().delete(backgroundCommand).where(eq(backgroundCommand.toolUseId, command.id)).run();
			opts.onEnd(command, result);
		})
		.catch((err: unknown) => {
			console.error(`[nolune] background command ${command.id} could not finish:`, err);
		});
	return Promise.resolve({ started: command });
}

/** Stops a conversation's background commands. Their output isn't handed over. */
export function stopBackgroundCommands(conversationId: string, byName: string): void {
	for (const c of backgroundCommands(conversationId)) {
		c.stoppedBy = byName;
		c.abort.abort();
	}
}

export type BackgroundCommandRow = typeof backgroundCommand.$inferSelect;

/** The conversation's background commands, as its rows have them, so any process can list them. */
export function listBackgroundCommands(conversationId: string): BackgroundCommandRow[] {
	return getDb()
		.select()
		.from(backgroundCommand)
		.where(eq(backgroundCommand.conversationId, conversationId))
		.orderBy(asc(backgroundCommand.startedAt))
		.all();
}

export class BackgroundError extends Error {}

/**
 * `nolune background stop`: asks the gateway to stop these of the conversation's background work,
 * which it does within seconds: commands by their id (the pid) and subagents by theirs. 'all' is
 * all of it. Nothing they would have handed over reaches the conversation. Returns what it asked
 * to stop; an id it doesn't know, or work that already ended, is an error and stops nothing.
 */
export function requestBackgroundStop(
	conversationId: string,
	ids: string[] | 'all',
	byName: string
): { commands: BackgroundCommandRow[]; subagents: Subagent[] } {
	const commands = listBackgroundCommands(conversationId).filter((c) => !c.stoppedBy);
	const subagents = activeSubagents(conversationId).filter((s) => s.status !== 'stopping');
	const chosen: { commands: BackgroundCommandRow[]; subagents: Subagent[] } =
		ids === 'all' ? { commands, subagents } : { commands: [], subagents: [] };
	// Every id is checked before anything is stopped.
	for (const id of ids === 'all' ? [] : new Set(ids)) {
		const command = commands.find((c) => c.pid !== null && String(c.pid) === id);
		const subagent = subagents.find((s) => s.name === id.toLowerCase());
		if (command) chosen.commands.push(command);
		else if (subagent) chosen.subagents.push(subagent);
		else throw new BackgroundError(notRunning(conversationId, id));
	}
	for (const c of chosen.commands) {
		getDb()
			.update(backgroundCommand)
			.set({ stoppedBy: byName })
			.where(eq(backgroundCommand.toolUseId, c.toolUseId))
			.run();
	}
	return {
		commands: chosen.commands,
		subagents: chosen.subagents.map((s) => {
			try {
				return requestSubagentStop({ parentId: conversationId, name: s.name });
			} catch (err) {
				if (err instanceof SubagentError) throw new BackgroundError(err.message);
				throw err;
			}
		})
	};
}

/** Why there's nothing to stop by that id. */
function notRunning(conversationId: string, id: string): string {
	const stopping =
		listBackgroundCommands(conversationId).find((c) => String(c.pid) === id && c.stoppedBy) ??
		(findSubagent(conversationId, id.toLowerCase())?.status === 'stopping' ? id : undefined);
	if (stopping) return `${id} is being stopped already.`;
	const subagent = findSubagent(conversationId, id.toLowerCase());
	if (subagent) return `${subagent.name} isn't working (${subagent.status}).`;
	return `nothing with the id "${id}" runs in this conversation's background. \`nolune background\` lists what does.`;
}

/**
 * Gateway only: stops the commands `nolune background stop` (or `nolune agent stop`, for its
 * watches) asked to. After every command the agent runs and at every tick, like processSubagents.
 */
export function processBackgroundStops(): void {
	const asked = getDb()
		.select()
		.from(backgroundCommand)
		.where(isNotNull(backgroundCommand.stoppedBy))
		.all();
	for (const row of asked) {
		const command = running.get(row.toolUseId);
		if (!command || command.stoppedBy) continue;
		command.stoppedBy = row.stoppedBy;
		command.abort.abort();
	}
}

/**
 * What runs in the conversation's background, a line or two each starting with its id: for
 * `nolune background`, and for the agent with a person's message (runner.ts).
 */
export function describeBackground(
	conversationId: string,
	opts: { now?: number; withLogs?: boolean } = {}
): string[] {
	const now = opts.now ?? Date.now();
	const lines: string[] = [];
	for (const c of listBackgroundCommands(conversationId)) {
		const state = c.stoppedBy
			? 'being stopped'
			: `running for ${ranFor(now - c.startedAt.getTime())}`;
		lines.push(`${c.pid ?? '?'}  command, ${state}${c.summary ? `: ${c.summary}` : ''}`);
		lines.push(`  $ ${firstLine(c.command)}`);
	}
	for (const s of activeSubagents(conversationId)) {
		const state =
			s.status === 'stopping'
				? 'being stopped'
				: s.status === 'pending'
					? 'starting'
					: `working for ${ranFor(now - s.updatedAt.getTime())}`;
		// Its chat is named after its id and its task.
		const title = getConversation(s.conversationId)?.title ?? '';
		const task = title.startsWith(`${s.name}: `) ? title.slice(s.name.length + 2) : title;
		lines.push(`${s.name}  subagent, ${state}${task ? `: ${task}` : ''}`);
		const log = opts.withLogs ? subagentLogPath(s) : null;
		if (log) lines.push(`  log: ${log}`);
	}
	return lines;
}

function firstLine(text: string, max = 120): string {
	const lines = text.trim().split('\n');
	const line = lines[0];
	return line.length > max || lines.length > 1 ? `${line.slice(0, max - 1)}…` : line;
}

/** "under a minute", "12 min", "2 h 5 min". */
function ranFor(ms: number): string {
	const minutes = Math.floor(Math.max(0, ms) / 60_000);
	if (minutes < 1) return 'under a minute';
	if (minutes < 60) return `${minutes} min`;
	const hours = Math.floor(minutes / 60);
	return minutes % 60 ? `${hours} h ${minutes % 60} min` : `${hours} h`;
}

/** Background commands a restart cut off: their rows are removed as they're returned. */
export function takeInterruptedBackgroundCommands(): (typeof backgroundCommand.$inferSelect)[] {
	const rows = getDb().select().from(backgroundCommand).all();
	const leftover = rows.filter((row) => !running.has(row.toolUseId));
	for (const row of leftover) {
		getDb().delete(backgroundCommand).where(eq(backgroundCommand.toolUseId, row.toolUseId)).run();
	}
	return leftover;
}
