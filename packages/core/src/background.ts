import { eq } from 'drizzle-orm';
import { getDb } from './db/index.ts';
import { backgroundCommand } from './db/schema.ts';
import { runCommand, type RunCommandInput, type RunCommandResult } from './run-command.ts';

/**
 * Commands the agent started with `run_in_background`. The runner answers the call at once and,
 * when the command ends, hands its output to the conversation as a queued message (runner.ts).
 * The processes live in the gateway; a row per command lets the next start tell the conversation
 * that a restart cut it off.
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

export function hasBackgroundCommands(conversationId: string): boolean {
	return backgroundCommands(conversationId).length > 0;
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
					command: opts.input.command
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

/** Background commands a restart cut off: their rows are removed as they're returned. */
export function takeInterruptedBackgroundCommands(): (typeof backgroundCommand.$inferSelect)[] {
	const rows = getDb().select().from(backgroundCommand).all();
	const leftover = rows.filter((row) => !running.has(row.toolUseId));
	for (const row of leftover) {
		getDb().delete(backgroundCommand).where(eq(backgroundCommand.toolUseId, row.toolUseId)).run();
	}
	return leftover;
}
