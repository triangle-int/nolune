import { chmodSync, mkdirSync, rmSync } from 'node:fs';
import { connect, createServer, type Server, type Socket } from 'node:net';
import { dirname, isAbsolute } from 'node:path';
import { paths } from '@btw/core';
import { createIo, type Io } from './io.ts';
import { LOCAL_ONLY, PROTOCOL, receive, send, type ClientMessage } from './protocol.ts';
import { runCli } from './run.ts';

/*
 * The gateway's side of protocol.ts: it runs `btw` commands for the CLI, on the btw it already
 * has loaded, so a command costs the client only Node's startup rather than loading all of btw.
 * Commands see the client's arguments, environment, folder and stdin through their io, never the
 * gateway's own (see io.ts).
 */

type Run = (argv: string[], io: Io) => Promise<number>;

function isRun(message: ClientMessage): message is Extract<ClientMessage, { type: 'run' }> {
	return (
		message.type === 'run' &&
		Array.isArray(message.argv) &&
		message.argv.every((arg) => typeof arg === 'string') &&
		typeof message.cwd === 'string' &&
		isAbsolute(message.cwd) &&
		typeof message.env === 'object' &&
		message.env !== null
	);
}

/** One connection, one command. The client hanging up stops it (io.signal). */
function handle(socket: Socket, run: Run): void {
	const abort = new AbortController();
	let started = false;
	let answerStdin: ((data: string) => void) | undefined;
	let stdin: Promise<string> | undefined;

	// A client that went away: 'close' follows, and an unhandled error would stop the gateway.
	socket.on('error', () => {});
	socket.on('close', () => abort.abort(new Error('the command was stopped')));
	receive<ClientMessage>(socket, (message) => {
		if (message.type === 'stdin') {
			answerStdin?.(typeof message.data === 'string' ? message.data : '');
			return;
		}
		if (started || !isRun(message)) {
			socket.destroy();
			return;
		}
		started = true;
		if (message.protocol !== PROTOCOL || LOCAL_ONLY.has(message.argv[0] ?? '')) {
			const reason = message.protocol !== PROTOCOL ? 'another protocol version' : 'runs locally';
			send(socket, { type: 'decline', reason });
			socket.end();
			return;
		}
		const io = createIo({
			stdout: (data) => send(socket, { type: 'stdout', data }),
			stderr: (data) => send(socket, { type: 'stderr', data }),
			env: message.env,
			cwd: message.cwd,
			// A person at a terminal runs their commands locally (see index.ts): none are here.
			stdinIsTTY: false,
			readStdin: () =>
				(stdin ??= new Promise((resolve, reject) => {
					if (abort.signal.aborted) return reject(abort.signal.reason);
					answerStdin = resolve;
					abort.signal.addEventListener('abort', () => reject(abort.signal.reason), {
						once: true
					});
					send(socket, { type: 'stdin' });
				})),
			signal: abort.signal
		});
		run(message.argv, io)
			.catch((err: unknown) => {
				console.error('[btw] a CLI command failed in the gateway:', err);
				return 1;
			})
			.then((code) => {
				send(socket, { type: 'exit', code });
				socket.end();
			});
	});
}

/** Whether something already answers on the socket: another gateway on the same btw home. */
function answers(socketPath: string): Promise<boolean> {
	return new Promise((resolve) => {
		const probe = connect(socketPath);
		probe.on('connect', () => {
			probe.destroy();
			resolve(true);
		});
		probe.on('error', () => resolve(false));
	});
}

/**
 * Serves `btw` commands on the socket, calling `run()` for the code to run each with (the latest,
 * after `pnpm dev` reloads it). Resolves with the server, or null when another gateway already
 * serves this btw home or the socket can't be made; the CLI then runs commands itself.
 */
export async function serveCommands(socketPath: string, run: () => Run): Promise<Server | null> {
	if (await answers(socketPath)) {
		console.log(`[btw] another gateway already runs btw commands on ${socketPath}`);
		return null;
	}
	const server = createServer((socket) => handle(socket, run()));
	try {
		// The folder is private before the socket exists in it: only this user can connect.
		const dir = dirname(socketPath);
		mkdirSync(dir, { recursive: true, mode: 0o700 });
		chmodSync(dir, 0o700);
		// Left by a gateway that didn't shut down cleanly: nothing answers on it.
		rmSync(socketPath, { force: true });
		await new Promise<void>((resolve, reject) => {
			server.once('error', reject);
			server.listen(socketPath, () => {
				server.off('error', reject);
				resolve();
			});
		});
		chmodSync(socketPath, 0o600);
	} catch (err) {
		console.error(
			`[btw] couldn't run btw commands on ${socketPath}; the CLI runs them itself:`,
			err
		);
		server.close();
		return null;
	}
	server.on('error', (err) => console.error('[btw] the btw command socket failed:', err));
	// It never keeps its process running by itself: the gateway exits once its HTTP server closes.
	server.unref();
	return server;
}

const holder = globalThis as unknown as { __btwCommands?: { run: Run } };

/**
 * The gateway's own command socket (paths.cliSocket), once per process. When `pnpm dev` reloads
 * this module, the running server keeps going with the new code.
 */
export function serveGatewayCommands(): void {
	if (holder.__btwCommands) {
		holder.__btwCommands.run = runCli;
		return;
	}
	const state = (holder.__btwCommands = { run: runCli });
	void serveCommands(paths.cliSocket, () => state.run).then((server) => {
		if (!server) return;
		const connections = new Set<Socket>();
		server.on('connection', (socket) => {
			connections.add(socket);
			socket.on('close', () => connections.delete(socket));
		});
		// adapter-node's graceful shutdown, once the HTTP server has closed: commands still running
		// (a `btw agent watch` waits for as long as its subagent works) are stopped, so they don't
		// keep the gateway alive, and their clients say the gateway stopped.
		process.once('sveltekit:shutdown', () => {
			server.close();
			for (const socket of connections) socket.destroy();
		});
		// So the next CLI finds nothing to connect to, rather than a socket nobody answers.
		process.once('exit', () => rmSync(paths.cliSocket, { force: true }));
	});
}
