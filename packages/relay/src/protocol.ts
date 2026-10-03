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

/**
 * Registration, status and release: `POST` here, `GET` and `DELETE` on `<path>/<name>`; and
 * notifications, `POST <path>/<name>/push`.
 */
export const GATEWAYS_PATH = '/api/gateways';

export type Hello = { type: 'hello'; protocol: number; name: string; token: string };
export type Ready = { type: 'ready'; url: string };

/** What `POST /api/gateways` answers. The token is shown only this once. */
export type Registration = { name: string; url: string; token: string };

/** What `GET /api/gateways/<name>` answers to the gateway's token. */
export type GatewayStatus = {
	name: string;
	url: string;
	online: boolean;
	/** What passed through the relay this month (UTC), and how much may; null: no limit. */
	traffic: { month: string; bytes: number; limit: number | null };
	/** Why the relay's operator blocked it, when they have. */
	blocked?: string;
};

/**
 * A notification for the family's iPhones (nolune for iOS, ios/): `POST <path>/<name>/push` with
 * the gateway's token. The relay sends it on to Apple, which only takes notifications for the
 * app from whoever holds its key.
 */
export const PUSH = 'push';

export type Push = {
	/** Each iPhone's token from Apple, in hex, and whether it's a development build's. */
	devices: { token: string; sandbox?: boolean }[];
	title: string;
	subtitle?: string;
	body: string;
	/** Notifications with the same thread are grouped on the lock screen: a profile's. */
	thread?: string;
	/** What the app opens when it's tapped: a path on the family's address. */
	path?: string;
};

/**
 * A Live Activity's change, on the lock screen and in the Dynamic Island of the family's iPhones:
 * a reply nolune is working on (nolune for iOS starts it). `POST <path>/<name>/activity` with the
 * gateway's token. Each activity has its own token from Apple, which the app gave its nolune.
 */
export const ACTIVITY = 'activity';

export type ActivityPush = {
	/** Each activity's token from Apple, in hex, and whether it's a development build's. */
	activities: { token: string; sandbox?: boolean }[];
	/** `update` while the work goes on; `end` once it's done. */
	event: 'update' | 'end';
	/** What the activity shows (the app's `ReplyActivity.State`). */
	state: ActivityState;
	/** When an ended activity leaves the lock screen, in seconds since 1970; Apple's 4 hours without. */
	dismissAt?: number;
};

export type ActivityState = {
	/** The chat's title. */
	title: string;
	/** What nolune does now ("Checking the forecast"), or the reply's first words once it's done. */
	step: string;
	running: boolean;
};

/** What `POST <path>/<name>/push` and `/activity` answer. */
export type PushResult = {
	/** How many iPhones Apple took it for. */
	sent: number;
	/** The tokens Apple says no app has any more: the gateway forgets them. */
	gone: string[];
};

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
	/** The gateway gave its name back (`DELETE /api/gateways/<name>`), or the operator removed it. */
	released: 4004,
	/** The relay's operator blocked the gateway; the reason is the close reason. */
	blocked: 4005
} as const;

/**
 * A gateway's name, the first label of its address: 3 to 32 lowercase letters, digits and dashes,
 * starting and ending with a letter or digit. No `--`, so no name reads as punycode (`xn--`).
 */
export function isValidName(name: string): boolean {
	return /^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$/.test(name) && !name.includes('--');
}
