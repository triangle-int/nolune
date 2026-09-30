/*
 * How a nolune gateway reaches the family through the relay, without a tunnel or an open port.
 *
 * The gateway registers once (`POST /api/gateways`) and gets a name, its public address
 * (https://<name>.<domain>) and a secret token. When it runs, it opens a WebSocket to the relay
 * (CONNECT_PATH) and says `hello` with the name and token in a text message; the relay answers
 * `ready`. From then on the binary messages carry an HTTP/2 connection in which the relay is the
 * client: each request someone makes to the public address becomes a stream to the gateway, which
 * passes it on to its own web server. HTTP/2 brings many requests at once over the one socket, and
 * flow control, so a large upload doesn't hold up an event stream. The relay closes the WebSocket
 * with one of the CLOSE codes when it won't carry a gateway.
 *
 * Nothing here imports anything beyond Node, so the gateway's side (packages/cli) can share it.
 */

/** Bumped when the messages change: the relay closes connections that speak another version. */
export const RELAY_PROTOCOL = 1;

/** Where a gateway opens its WebSocket, on the relay's own host. */
export const CONNECT_PATH = '/api/connect';

/** Registration, status and release: `POST` here, `GET` and `DELETE` on `<path>/<name>`. */
export const GATEWAYS_PATH = '/api/gateways';

export type Hello = { type: 'hello'; protocol: number; name: string; token: string };
export type Ready = { type: 'ready'; url: string };

/** What `POST /api/gateways` answers. The token is shown only this once. */
export type Registration = { name: string; url: string; token: string };

/** What `GET /api/gateways/<name>` answers to the gateway's token. */
export type GatewayStatus = { name: string; url: string; online: boolean };

/** Why the relay closed a gateway's WebSocket (application close codes, 4000-4999). */
export const CLOSE = {
	/** No `hello` in time, or not a valid one. */
	noHello: 4000,
	/** No gateway of that name, or the wrong token. */
	unknown: 4001,
	/** The gateway speaks another RELAY_PROTOCOL. */
	protocol: 4002,
	/** Another connection with the same name and token took over. */
	replaced: 4003,
	/** The gateway gave its name back (`DELETE /api/gateways/<name>`). */
	released: 4004
} as const;

/**
 * A gateway's name, the first label of its address: 3 to 32 lowercase letters, digits and dashes,
 * starting and ending with a letter or digit. No `--`, so no name reads as punycode (`xn--`).
 */
export function isValidName(name: string): boolean {
	return /^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$/.test(name) && !name.includes('--');
}
