import { Duplex } from 'node:stream';

/*
 * A WebSocket as a byte stream, for the HTTP/2 connection the relay and a gateway run over it
 * (protocol.ts). Both ends use it: the relay with the `ws` package's socket and the gateway with
 * Node's built-in WebSocket, so it keeps to the API they share.
 */

/** What this needs of a WebSocket: the part `ws` and the WHATWG WebSocket have in common. */
export interface MessageSocket {
	readonly readyState: number;
	readonly bufferedAmount: number;
	binaryType: string;
	send(data: Uint8Array<ArrayBuffer>): void;
	close(code?: number, reason?: string): void;
	addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
	addEventListener(type: 'close', listener: () => void): void;
}

const OPEN = 1;

/** Past this much unsent, writes wait for the socket to catch up. */
const HIGH_WATER_MARK = 1024 * 1024;

/**
 * Binary messages become the stream's bytes; text messages go to `onText` (protocol messages, never
 * part of the stream). Reading can't pause the socket, but it needn't: HTTP/2's flow control
 * bounds what the other side sends. Writing waits while the socket has too much unsent. Ending or
 * destroying the stream closes the socket, and the socket closing ends the stream's reading side.
 */
export function webSocketStream(socket: MessageSocket, onText?: (text: string) => void): Duplex {
	socket.binaryType = 'arraybuffer';
	const whenDrained = (callback: (err?: Error | null) => void) => {
		if (socket.readyState !== OPEN) return callback(new Error('the WebSocket closed'));
		if (socket.bufferedAmount < HIGH_WATER_MARK) return callback();
		setTimeout(() => whenDrained(callback), 5);
	};
	const stream = new Duplex({
		read() {},
		write(chunk: Buffer, _encoding, callback) {
			if (socket.readyState !== OPEN) return callback(new Error('the WebSocket closed'));
			// A Buffer's memory is never shared, which the WHATWG WebSocket's type asks for.
			socket.send(chunk as Uint8Array<ArrayBuffer>);
			whenDrained(callback);
		},
		final(callback) {
			socket.close(1000);
			callback();
		},
		destroy(err, callback) {
			if (socket.readyState === OPEN) socket.close(err ? 1011 : 1000);
			callback(err);
		}
	});
	socket.addEventListener('message', ({ data }) => {
		if (typeof data === 'string') onText?.(data);
		else if (data instanceof ArrayBuffer) stream.push(Buffer.from(data));
	});
	// What arrived before is still read; writes from now on fail (whenDrained, write).
	socket.addEventListener('close', () => stream.push(null));
	return stream;
}
