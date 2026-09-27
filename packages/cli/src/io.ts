import { format } from 'node:util';

/*
 * What a command reads and writes besides its arguments: its output, environment, working folder
 * and stdin. Commands take it instead of reaching for `process`, so the same code can run in a
 * process of its own, as `btw` does today, or inside another one on someone else's behalf (the
 * gateway, for the agent's commands: see issue #42), where `process` is the gateway's.
 */
export interface Io {
	/** Standard output, as given; `log` adds the newline. */
	stdout(text: string): void;
	stderr(text: string): void;
	/** Like console.log and console.error, on this command's output. */
	log(...args: unknown[]): void;
	error(...args: unknown[]): void;
	/** The command's environment: BTW_PROFILE, BTW_CONVERSATION_ID, BTW_VIEW_DIR, … */
	env: Readonly<Record<string, string | undefined>>;
	/** Where relative paths start. */
	cwd: string;
	/** True when stdin is a person at a terminal rather than something piped in. */
	stdinIsTTY: boolean;
	/** All of stdin, as it came. */
	readStdin(): Promise<string>;
	/** Aborted when the command should stop early. */
	signal: AbortSignal;
}

export type IoHost = Omit<Io, 'log' | 'error'>;

export function createIo(host: IoHost): Io {
	return {
		...host,
		log: (...args) => host.stdout(`${format(...args)}\n`),
		error: (...args) => host.stderr(`${format(...args)}\n`)
	};
}

/** This process's own streams, environment and folder: `btw` run from a shell or a command. */
export function processIo(): Io {
	return createIo({
		stdout: (text) => process.stdout.write(text),
		stderr: (text) => process.stderr.write(text),
		env: process.env,
		cwd: process.cwd(),
		stdinIsTTY: !!process.stdin.isTTY,
		readStdin: async () => {
			let data = '';
			for await (const chunk of process.stdin) data += chunk;
			return data;
		},
		// Ctrl+C ends the whole process, so nothing needs to abort from inside it.
		signal: new AbortController().signal
	});
}

/** A mistake in how the command was called, or something it can't do, in words for the caller. */
export function fail(message: string): never {
	throw new Error(message);
}
