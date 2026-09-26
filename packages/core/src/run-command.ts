import { spawn } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { isAbsolute, join, resolve } from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import type Anthropic from '@anthropic-ai/sdk';
import { readConfig } from './config.ts';
import { paths } from './paths.ts';

export const DEFAULT_TIMEOUT_SECONDS = 120;
export const MAX_TIMEOUT_SECONDS = 1800;
/** Background commands: nothing waits on them, so they may run for much longer. */
export const DEFAULT_BACKGROUND_TIMEOUT_SECONDS = 3600;
export const MAX_BACKGROUND_TIMEOUT_SECONDS = 86_400;
const HEAD_CHARS = 10_000;
const TAIL_CHARS = 20_000;
const KILL_GRACE_MS = 2000;

/**
 * The first `run_command`, which conversations from before tools were saved with each chat still
 * send (LEGACY_TOOLS). It must never change: it is part of their cached prefix, and their thinking
 * blocks are bound to it.
 */
export const RUN_COMMAND_TOOL_V1: Anthropic.Tool = {
	name: 'run_command',
	description:
		'Run a shell command on this computer and return its combined stdout and stderr plus the exit code. Each call is a fresh login shell with no keyboard input. Output longer than 30,000 characters is cut in the middle.',
	input_schema: {
		type: 'object',
		// `summary` and `icon` come first so they stream in before the command: the web UI shows
		// them in place of the command for people who don't read shell.
		properties: {
			summary: {
				type: 'string',
				description:
					'What this command does, in a few plain words for someone who doesn\'t read shell, in the language of the conversation. Starts with a verb in the -ing form, no jargon, no file paths unless they matter to the person, e.g. "Checking tomorrow\'s weather" or "Looking for the tax PDF".'
			},
			icon: {
				type: 'string',
				description:
					'A Lucide icon name (lucide.dev/icons, kebab-case) that fits the summary, e.g. calendar, mail, cloud-sun, file-search, folder-open, globe, printer, image, music, trash-2, clock, terminal.'
			},
			command: { type: 'string', description: 'The shell command to run.' },
			cwd: {
				type: 'string',
				description:
					'Working folder. Relative paths and ~ are resolved; defaults to the profile folder.'
			},
			timeout_seconds: {
				type: 'integer',
				description: `Seconds before the command is killed. Default ${DEFAULT_TIMEOUT_SECONDS}, max ${MAX_TIMEOUT_SECONDS}.`
			}
		},
		required: ['summary', 'icon', 'command'],
		additionalProperties: false
	}
};

const v1Properties = (RUN_COMMAND_TOOL_V1.input_schema.properties ?? {}) as Record<
	string,
	{ type: string; description: string }
>;

/** `run_command` as new conversations get it: it can also run a command in the background. */
export const RUN_COMMAND_TOOL: Anthropic.Tool = {
	name: 'run_command',
	description:
		'Run a shell command on this computer and return its combined stdout and stderr plus the exit code. Each call is a fresh login shell with no keyboard input. Output longer than 30,000 characters is cut in the middle. With run_in_background, the call returns at once and the output comes back in a message of its own when the command ends.',
	input_schema: {
		type: 'object',
		properties: {
			summary: v1Properties.summary,
			icon: v1Properties.icon,
			command: v1Properties.command,
			cwd: v1Properties.cwd,
			timeout_seconds: {
				type: 'integer',
				description: `Seconds before the command is killed. Default ${DEFAULT_TIMEOUT_SECONDS}, max ${MAX_TIMEOUT_SECONDS}; in the background, default ${DEFAULT_BACKGROUND_TIMEOUT_SECONDS}, max ${MAX_BACKGROUND_TIMEOUT_SECONDS}.`
			},
			run_in_background: {
				type: 'boolean',
				description:
					'Start the command and return at once, for long jobs you don\'t need to wait on, like a big download or `btw agent watch`. Keep working or end your turn: when the command ends, its output arrives as a message that starts with "[Background command finished".'
			}
		},
		required: ['summary', 'icon', 'command'],
		additionalProperties: false
	}
};

/** What new conversations are created with, and save (`conversation.tools`). */
export const TOOLS: Anthropic.Tool[] = [RUN_COMMAND_TOOL];
/** What conversations created before tools were saved with each chat send. */
export const LEGACY_TOOLS: Anthropic.Tool[] = [RUN_COMMAND_TOOL_V1];

export interface RunCommandInput {
	command: string;
	cwd?: string;
	timeoutSeconds?: number;
	/** Return at once and hand the output over when the command ends (see background.ts). */
	background?: boolean;
}

/**
 * Validates model-supplied input. Returns an error message for the model on failure. `summary`
 * and `icon` are only for display, so a missing one never stops the command.
 */
export function parseRunCommandInput(input: unknown): RunCommandInput | string {
	if (!input || typeof input !== 'object') return 'Input must be an object.';
	const { command, cwd, timeout_seconds, run_in_background } = input as Record<string, unknown>;
	if (typeof command !== 'string' || !command.trim())
		return '`command` must be a non-empty string.';
	if (cwd !== undefined && typeof cwd !== 'string') return '`cwd` must be a string.';
	if (
		timeout_seconds !== undefined &&
		(typeof timeout_seconds !== 'number' || timeout_seconds <= 0)
	) {
		return '`timeout_seconds` must be a positive number.';
	}
	if (run_in_background !== undefined && typeof run_in_background !== 'boolean') {
		return '`run_in_background` must be true or false.';
	}
	return {
		command,
		cwd: cwd as string | undefined,
		timeoutSeconds: timeout_seconds as number | undefined,
		...(run_in_background ? { background: true } : {})
	};
}

/** Seconds a command may run before it's killed: what it asked for, within its limit. */
export function commandTimeout(input: RunCommandInput): number {
	return input.background
		? Math.min(
				input.timeoutSeconds ?? DEFAULT_BACKGROUND_TIMEOUT_SECONDS,
				MAX_BACKGROUND_TIMEOUT_SECONDS
			)
		: Math.min(input.timeoutSeconds ?? DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS);
}

export function commandShell(): string {
	return process.env.SHELL || '/bin/zsh';
}

const SECRET_ENV = [
	'ANTHROPIC_API_KEY',
	'ANTHROPIC_AUTH_TOKEN',
	'BETTER_AUTH_SECRET',
	'BTW_AUTH_SECRET',
	'DATABASE_URL'
];

export function commandEnv(extra: Record<string, string>): NodeJS.ProcessEnv {
	const env: NodeJS.ProcessEnv = { ...process.env };
	for (const key of SECRET_ENV) delete env[key];
	// Commands get NO_COLOR; an inherited FORCE_COLOR would contradict it (and makes node warn).
	delete env.FORCE_COLOR;
	let configured: Record<string, string> = {};
	try {
		configured = readConfig().commandEnv ?? {};
	} catch {
		// no config yet
	}
	return {
		...env,
		...configured,
		PATH: `${paths.bin}:${env.PATH ?? '/usr/bin:/bin:/usr/sbin:/sbin'}`,
		BTW_HOME: paths.home,
		TERM: 'dumb',
		NO_COLOR: '1',
		PAGER: 'cat',
		GIT_PAGER: 'cat',
		...extra
	};
}

function resolveCwd(cwd: string | undefined, base: string): string {
	if (!cwd) return base;
	if (cwd === '~') return homedir();
	if (cwd.startsWith('~/')) return join(homedir(), cwd.slice(2));
	return isAbsolute(cwd) ? cwd : resolve(base, cwd);
}

// eslint-disable-next-line no-control-regex
const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*(\x07|\x1b\\)/g;

class CappedOutput {
	head = '';
	tail = '';
	dropped = 0;

	push(text: string): void {
		if (this.head.length < HEAD_CHARS) {
			const room = HEAD_CHARS - this.head.length;
			this.head += text.slice(0, room);
			text = text.slice(room);
		}
		if (!text) return;
		this.tail += text;
		if (this.tail.length > TAIL_CHARS) {
			this.dropped += this.tail.length - TAIL_CHARS;
			this.tail = this.tail.slice(-TAIL_CHARS);
		}
	}

	toString(): string {
		const middle = this.dropped ? `\n\n[... ${this.dropped} characters cut ...]\n\n` : '';
		return (this.head + middle + this.tail).replace(ANSI, '');
	}
}

/** Process groups of commands still running, killed if the gateway exits (e.g. service restart). */
const runningGroups = new Set<number>();
let exitHookInstalled = false;

function trackGroup(pid: number): void {
	runningGroups.add(pid);
	if (exitHookInstalled) return;
	exitHookInstalled = true;
	process.once('exit', () => {
		for (const group of runningGroups) {
			try {
				process.kill(-group, 'SIGTERM');
			} catch {
				// already gone
			}
		}
	});
}

export interface RunCommandResult {
	/** Text sent back to the model. */
	content: string;
	isError: boolean;
	/** Null when the command didn't exit by itself (not started, timed out, stopped, killed). */
	exitCode: number | null;
}

export function runCommand(
	input: RunCommandInput,
	options: {
		defaultCwd: string;
		env: NodeJS.ProcessEnv;
		signal: AbortSignal;
		onOutput?: (chunk: string) => void;
		/** Called once the shell is started, with its pid, which is also its process group. */
		onStart?: (pid: number) => void;
		/** Text used when the command is aborted, e.g. "Stopped by Anna." */
		abortReason: () => string;
	}
): Promise<RunCommandResult> {
	const cwd = resolveCwd(input.cwd, options.defaultCwd);
	if (!existsSync(cwd) || !statSync(cwd).isDirectory()) {
		return Promise.resolve({
			content: `Working folder does not exist: ${cwd}`,
			isError: true,
			exitCode: null
		});
	}
	const timeoutSeconds = commandTimeout(input);

	return new Promise((done) => {
		const child = spawn(commandShell(), ['-lc', input.command], {
			cwd,
			env: options.env,
			stdio: ['ignore', 'pipe', 'pipe'],
			// Own process group, so a timeout or Stop can kill everything the command started.
			detached: true
		});
		if (child.pid) {
			trackGroup(child.pid);
			options.onStart?.(child.pid);
		}
		const output = new CappedOutput();
		const decoders = [new StringDecoder('utf8'), new StringDecoder('utf8')];
		let ended: 'timeout' | 'aborted' | null = null;
		let finished = false;

		const onData = (decoder: StringDecoder) => (buf: Buffer) => {
			const text = decoder.write(buf);
			if (!text) return;
			output.push(text);
			options.onOutput?.(text);
		};
		child.stdout.on('data', onData(decoders[0]));
		child.stderr.on('data', onData(decoders[1]));

		const killGroup = (sig: NodeJS.Signals) => {
			try {
				if (child.pid) process.kill(-child.pid, sig);
			} catch {
				// already gone
			}
		};
		const terminate = (why: 'timeout' | 'aborted') => {
			if (ended || finished) return;
			ended = why;
			killGroup('SIGTERM');
			setTimeout(() => killGroup('SIGKILL'), KILL_GRACE_MS).unref();
		};
		const timer = setTimeout(() => terminate('timeout'), timeoutSeconds * 1000);
		const onAbort = () => terminate('aborted');
		options.signal.addEventListener('abort', onAbort, { once: true });
		if (options.signal.aborted) onAbort();

		const finish = (code: number | null, sig: NodeJS.Signals | null, spawnError?: Error) => {
			if (finished) return;
			finished = true;
			if (child.pid) runningGroups.delete(child.pid);
			clearTimeout(timer);
			options.signal.removeEventListener('abort', onAbort);
			for (const d of decoders) output.push(d.end());
			// Background processes may still hold the pipes open; stop reading from them.
			child.stdout.destroy();
			child.stderr.destroy();

			let text = output.toString().trimEnd() || '(no output)';
			let isError = false;
			if (spawnError) {
				text = `Failed to start the command: ${spawnError.message}`;
				isError = true;
			} else if (ended === 'timeout') {
				text += `\n[timed out after ${timeoutSeconds}s, the command was killed]`;
				isError = true;
			} else if (ended === 'aborted') {
				text += `\n[${options.abortReason()}]`;
				isError = true;
			} else {
				text += sig ? `\n[killed by ${sig}]` : `\n[exit code ${code}]`;
			}
			done({ content: text, isError, exitCode: ended || spawnError || sig ? null : code });
		};

		child.on('error', (err) => finish(null, null, err));
		// 'exit' rather than 'close': a detached background child can keep the pipes open forever.
		child.on('exit', (code, sig) => setTimeout(() => finish(code, sig), 100));
	});
}
