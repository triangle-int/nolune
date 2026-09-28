import type { Socket } from 'node:net';

/*
 * How `nolune` hands a command to the gateway, where all of nolune is already loaded, over a Unix
 * socket only this user can open (paths.cliSocket). Messages are JSON, one per line. The client
 * sends `run`; the gateway streams the command's output back and ends with `exit`, or answers
 * `decline`, and the client then runs the command itself. Stdin goes over only when the command
 * reads it, so a command that doesn't never waits on it.
 */

/** Bumped when the messages change: a gateway that speaks another version declines. */
export const PROTOCOL = 1;

/**
 * Commands that always run in a process of their own: they prompt at a terminal, run the
 * gateway or manage its service.
 */
export const LOCAL_ONLY: ReadonlySet<string> = new Set(['setup', 'start', 'service', 'init']);

/** Larger messages end the connection: stdin is the only big one, and notes are far smaller. */
const MAX_MESSAGE_CHARS = 16 * 1024 * 1024;

export type ClientMessage =
	| {
			type: 'run';
			protocol: number;
			argv: string[];
			cwd: string;
			env: Record<string, string | undefined>;
	  }
	| { type: 'stdin'; data: string };

export type GatewayMessage =
	| { type: 'stdout' | 'stderr'; data: string }
	/** The command reads stdin: the client answers with all of it. */
	| { type: 'stdin' }
	| { type: 'exit'; code: number }
	| { type: 'decline'; reason: string };

/** Sends a message, unless the other side has gone. */
export function send(socket: Socket, message: ClientMessage | GatewayMessage): void {
	if (socket.writable) socket.write(`${JSON.stringify(message)}\n`);
}

/** Calls `onMessage` with each message as it arrives. Anything malformed ends the connection. */
export function receive<T>(socket: Socket, onMessage: (message: T) => void): void {
	let buffer = '';
	socket.setEncoding('utf8');
	socket.on('data', (chunk: string) => {
		buffer += chunk;
		for (let end = buffer.indexOf('\n'); end !== -1; end = buffer.indexOf('\n')) {
			if (socket.destroyed) return;
			const line = buffer.slice(0, end);
			buffer = buffer.slice(end + 1);
			let message: unknown;
			try {
				message = JSON.parse(line);
			} catch {
				socket.destroy();
				return;
			}
			if (typeof message !== 'object' || message === null) {
				socket.destroy();
				return;
			}
			try {
				onMessage(message as T);
			} catch (err) {
				// Thrown from a socket's event handler, it would end the whole process (the gateway).
				console.error('[nolune] a malformed message on the nolune command socket:', err);
				socket.destroy();
				return;
			}
		}
		if (buffer.length > MAX_MESSAGE_CHARS) socket.destroy();
	});
}
