import { connect } from 'node:net';
import {
	LOCAL_ONLY,
	PROTOCOL,
	receive,
	send,
	type ClientMessage,
	type GatewayMessage
} from './protocol.ts';

/*
 * The CLI's side of protocol.ts. It imports nothing of nolune's, so asking the gateway costs only
 * Node's own startup.
 */

export interface Stdio {
	stdout(text: string): void;
	stderr(text: string): void;
	readStdin(): Promise<string>;
}

/**
 * Runs a command in the gateway and resolves with its exit code, or with null when the gateway
 * doesn't run it: it isn't running, it speaks another protocol version, or the command stays
 * local. The caller then runs it itself. Once the gateway has the command, it's never run a
 * second time here, even if the gateway goes away in the middle: it may have done part of it.
 */
export function runInGateway(
	argv: string[],
	opts: {
		socketPath: string;
		cwd: string;
		env: Record<string, string | undefined>;
		stdio: Stdio;
	}
): Promise<number | null> {
	if (LOCAL_ONLY.has(argv[0] ?? '')) return Promise.resolve(null);
	return new Promise((resolve) => {
		const socket = connect(opts.socketPath);
		let connected = false;
		let result: number | null | undefined;
		const finish = (code: number | null) => {
			if (result !== undefined) return;
			result = code;
			socket.end();
			resolve(code);
		};

		socket.on('connect', () => {
			connected = true;
			const run: ClientMessage = {
				type: 'run',
				protocol: PROTOCOL,
				argv,
				cwd: opts.cwd,
				env: opts.env
			};
			send(socket, run);
		});
		// Not listening (no socket, or one left by a gateway that stopped): run it here.
		socket.on('error', () => {
			if (!connected) finish(null);
		});
		socket.on('close', () => {
			if (!connected || result !== undefined) return;
			opts.stdio.stderr('nolune: the gateway stopped before the command finished.\n');
			finish(1);
		});
		receive<GatewayMessage>(socket, (message) => {
			switch (message.type) {
				case 'stdout':
					opts.stdio.stdout(message.data);
					break;
				case 'stderr':
					opts.stdio.stderr(message.data);
					break;
				case 'stdin':
					opts.stdio.readStdin().then(
						(data) => send(socket, { type: 'stdin', data }),
						// Unreadable stdin reads as empty, as it would for the command itself.
						() => send(socket, { type: 'stdin', data: '' })
					);
					break;
				case 'exit':
					finish(message.code);
					break;
				case 'decline':
					finish(null);
					break;
			}
		});
	});
}
