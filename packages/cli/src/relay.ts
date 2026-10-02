import http from 'node:http';
import http2, {
	type IncomingHttpHeaders as Http2Headers,
	type ServerHttp2Session,
	type ServerHttp2Stream
} from 'node:http2';
import { parseArgs } from 'node:util';
import {
	DEFAULT_RELAY_SERVER,
	publicOrigin,
	readConfig,
	updateConfig,
	type RelayConfig
} from '@nolune/core';
import { endToEnd } from '@nolune/relay/headers';
import {
	CLOSE,
	CONNECT_PATH,
	GATEWAYS_PATH,
	RELAY_PROTOCOL,
	isValidName,
	type GatewayStatus,
	type Hello,
	type Ready,
	type Registration
} from '@nolune/relay/protocol';
import { webSocketStream } from '@nolune/relay/stream';
import { fail, type Io } from './io.ts';

/*
 * The gateway's side of the relay (packages/relay): an address that works from anywhere, without a
 * tunnel, an open port or a domain of the family's own. `nolune relay enable` registers with the
 * relay and keeps the name, address and token in config.json; `nolune start` then keeps a
 * WebSocket open to the relay and passes each request that comes over it to its own web server.
 */

export const RELAY_HELP = `Reaching nolune from anywhere (no tunnel, port forwarding or domain of your own)
  nolune relay enable [--name NAME]             get a public address through nolune's relay, like
                                             https://NAME.nolune.family (a random name without
                                             --name). It passes the family's traffic to this
                                             computer, and could see it, as any tunnel could.
                                             --server URL uses a relay of your own
  nolune relay status                           the address, and whether the gateway is connected
  nolune relay disable                          stop using the relay and give the address back`;

const NAME_RULE =
	'a name has 3 to 32 lowercase letters, digits and single dashes, and starts and ends with a letter or digit';

const RESTART_HINT =
	'Restart the gateway to apply it: `nolune service restart`, or stop `nolune start` and run it again.';

/** The relay couldn't be reached at all: no answer, rather than a refusal. */
export class RelayUnreachable extends Error {}

/** What the relay said when it refused, and its status. */
class RelayRefusal extends Error {
	status: number;
	constructor(message: string, status: number) {
		super(message);
		this.status = status;
	}
}

function size(bytes: number): string {
	const gb = bytes / 1024 ** 3;
	if (gb >= 1) return `${gb >= 10 ? Math.round(gb) : Number(gb.toFixed(1))} GB`;
	return `${Math.round(bytes / 1024 ** 2)} MB`;
}

/** A request to the relay's API, with its error in words when it refuses. */
async function relayApi<T>(server: string, path: string, init: RequestInit = {}): Promise<T> {
	let res: Response;
	try {
		res = await fetch(new URL(path, server), { ...init, signal: AbortSignal.timeout(15_000) });
	} catch (err) {
		const cause = (err as { cause?: { message?: string } }).cause?.message;
		throw new RelayUnreachable(
			`couldn't reach the relay at ${server} (${cause ?? (err as Error).message})`,
			{ cause: err }
		);
	}
	if (res.status === 204) return undefined as T;
	const body = (await res.json().catch(() => null)) as (T & { error?: string }) | null;
	if (!res.ok || !body) {
		throw new RelayRefusal(
			body?.error ?? `the relay at ${server} answered ${res.status}`,
			res.status
		);
	}
	return body;
}

export function registerGateway(server: string, name?: string): Promise<Registration> {
	return relayApi(server, GATEWAYS_PATH, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(name ? { name } : {})
	});
}

function gatewayPath(relay: RelayConfig): string {
	return `${GATEWAYS_PATH}/${encodeURIComponent(relay.name)}`;
}

export function gatewayStatus(relay: RelayConfig): Promise<GatewayStatus> {
	return relayApi(relay.server, gatewayPath(relay), {
		headers: { authorization: `Bearer ${relay.token}` }
	});
}

export function releaseGateway(relay: RelayConfig): Promise<void> {
	return relayApi(relay.server, gatewayPath(relay), {
		method: 'DELETE',
		headers: { authorization: `Bearer ${relay.token}` }
	});
}

/**
 * Gets an address from the relay and keeps it in config.json, unless there's one already (from
 * this relay, and with this name when one is asked for) that the relay still knows. A new name
 * gives the old one back; an address the relay forgot is asked for again.
 */
export async function enableRelay(
	io: Io,
	opts: { name?: string; server?: string } = {}
): Promise<{ relay: RelayConfig; created: boolean }> {
	const current = readConfig().relay;
	const server = (
		opts.server ||
		io.env.NOLUNE_RELAY_SERVER ||
		current?.server ||
		DEFAULT_RELAY_SERVER
	).replace(/\/+$/, '');
	let name = opts.name?.trim().toLowerCase() || undefined;
	if (name !== undefined && !isValidName(name)) fail(NAME_RULE);
	if (current && current.server === server && (name === undefined || name === current.name)) {
		// A relay that can't be reached now may well still know it.
		const known = await gatewayStatus(current).then(
			() => true,
			(err: unknown) => !(err instanceof RelayRefusal && err.status === 401)
		);
		if (known) return { relay: current, created: false };
		name = current.name;
	}
	const registration = await registerGateway(server, name);
	const relay: RelayConfig = {
		server,
		name: registration.name,
		url: registration.url,
		token: registration.token
	};
	updateConfig((c) => {
		c.relay = relay;
	});
	// Nothing else has the old token: an address that couldn't be given back just stays unused.
	if (current && (current.server !== server || current.name !== relay.name)) {
		await releaseGateway(current).catch(() => {});
	}
	return { relay, created: true };
}

export async function relayCommand(io: Io, action: string | undefined, args: string[]) {
	switch (action) {
		case 'enable': {
			const { values } = parseArgs({
				args,
				options: { name: { type: 'string' }, server: { type: 'string' } }
			});
			const { relay, created } = await enableRelay(io, values);
			if (!created) {
				io.log(`The relay is already on: ${relay.url}`);
				return;
			}
			io.log(`nolune's address: ${relay.url}
${RESTART_HINT} Then open the address on any device: everyone signs in with their own account.

The relay passes traffic between browsers and this computer, and could see it, as any tunnel
could. \`nolune relay disable\` stops using it.`);
			return;
		}
		case 'status': {
			const relay = readConfig().relay;
			if (!relay) {
				io.log(`The relay is off; nolune's address is ${publicOrigin(readConfig())}.
\`nolune relay enable\` gives it one that works from anywhere.`);
				return;
			}
			io.log(`Address  ${relay.url}`);
			io.log(`Relay    ${relay.server}`);
			let status: GatewayStatus;
			try {
				status = await gatewayStatus(relay);
			} catch (err) {
				io.log(
					err instanceof RelayRefusal && err.status === 401
						? "Gateway  the relay doesn't know this address any more: nolune asks for it again when it connects, or `nolune relay enable` now"
						: `Gateway  unknown: ${(err as Error).message}`
				);
				return;
			}
			const { online, traffic, blocked } = status;
			io.log(
				`Gateway  ${online ? 'connected' : 'not connected: is nolune running? (`nolune service status`)'}`
			);
			// Before the relay counted traffic, it didn't say.
			if (traffic) {
				const limit = traffic.limit === null ? '' : ` of ${size(traffic.limit)}`;
				const over = traffic.limit !== null && traffic.bytes >= traffic.limit;
				io.log(
					`Traffic  ${size(traffic.bytes)}${limit} this month${over ? ": used up, so the address doesn't work until the 1st" : ''}`
				);
			}
			if (blocked) io.log(`Blocked  by the relay: ${blocked}`);
			return;
		}
		case 'disable': {
			const relay = readConfig().relay;
			if (!relay) {
				io.log('The relay is already off.');
				return;
			}
			let note = '';
			try {
				await releaseGateway(relay);
			} catch (err) {
				note = `\nThe relay couldn't be told (${(err as Error).message}), but only this computer had the address's token, so nothing else can use it.`;
			}
			const config = updateConfig((c) => {
				delete c.relay;
			});
			io.log(`Stopped using the relay: ${relay.url} no longer leads here, and nolune's address is ${publicOrigin(config)}.
The nolune app for iPhone gets no notifications without it.
${RESTART_HINT}${note}`);
			return;
		}
		default:
			fail('usage: nolune relay enable [--name NAME] [--server URL] | status | disable');
	}
}

// --- The gateway's connection ---

export interface RelayLink {
	/** Closes the connection and the requests it carries, for good. */
	close(): void;
}

const READY_TIMEOUT_MS = 30_000;
const PING_MS = 30_000;
const PING_TIMEOUT_MS = 15_000;
const MAX_BACKOFF_MS = 60_000;
/** When the relay doesn't know the gateway and it couldn't get its name back: try again later. */
const UNKNOWN_RETRY_MS = 5 * 60_000;
/** Asking for the name again counts as a registration, which the relay limits: once an hour. */
const RECLAIM_EVERY_MS = 60 * 60_000;
/** As the relay's (packages/relay/src/relay.ts): uploads don't crawl over a long distance. */
const STREAM_WINDOW_BYTES = 1024 * 1024;
const SESSION_WINDOW_BYTES = 16 * 1024 * 1024;

/**
 * Keeps the gateway connected to the relay while it runs, reconnecting whenever the connection
 * drops (after a moment, then less often while the relay can't be reached), and passes every
 * request to the gateway's own web server at `target`. `log` gets a line when it connects, and
 * once when it loses the connection or can't get one, rather than at every try.
 */
export function connectRelay(
	relay: RelayConfig,
	target: { host: string; port: number },
	log: (message: string) => void
): RelayLink {
	const agent = new http.Agent({ keepAlive: true });
	const hello: Hello = {
		type: 'hello',
		protocol: RELAY_PROTOCOL,
		name: relay.name,
		token: relay.token
	};
	let stopped = false;
	let retry: NodeJS.Timeout | undefined;
	let endAttempt: (() => void) | undefined;
	let failures = 0;
	let reported = false;
	let wasOnline = false;
	let reclaimedAt = 0;

	/** config.json's address, when it's still the one this connection is for. */
	const sameAddress = (c: RelayConfig | undefined): c is RelayConfig =>
		c?.server === relay.server && c.name === relay.name;

	/**
	 * The relay doesn't know the address: it let the name go while nolune was away for months (or
	 * its operator removed it). Asks for the same name again, and keeps the new token in
	 * config.json. When someone else has the name now, nolune stops using it, so nothing opens their
	 * nolune for this one: 'stop'. 'later' when the relay couldn't be asked, or was asked lately.
	 */
	async function reclaim(): Promise<'back' | 'stop' | 'later'> {
		const current = readConfig().relay;
		if (!sameAddress(current)) {
			log(
				'the relay address in config.json changed, so nolune stopped using this one. ' +
					RESTART_HINT
			);
			return 'stop';
		}
		// `nolune relay enable` asked for it again already.
		if (current.token !== hello.token) {
			hello.token = current.token;
			return 'back';
		}
		if (Date.now() - reclaimedAt < RECLAIM_EVERY_MS) return 'later';
		reclaimedAt = Date.now();
		try {
			const registration = await registerGateway(relay.server, relay.name);
			hello.token = registration.token;
			updateConfig((c) => {
				if (sameAddress(c.relay)) c.relay = { ...c.relay, token: registration.token };
			});
			log(`the relay had let ${relay.url} go while nolune was away, so nolune asked for it again`);
			return 'back';
		} catch (err) {
			if (err instanceof RelayRefusal && err.status === 409) {
				updateConfig((c) => {
					if (sameAddress(c.relay)) delete c.relay;
				});
				log(
					`the relay let ${relay.url} go while nolune was away, and someone else has it now, so nolune stopped using it. \`nolune relay enable\` gets a new address; then restart nolune.`
				);
				return 'stop';
			}
			if (!reported) {
				log(
					`the relay doesn't know this address (${relay.name}) any more, and nolune couldn't ask for it again (${(err as Error).message}). It tries again every hour, or run \`nolune relay enable\` and restart nolune.`
				);
			}
			reported = true;
			return 'later';
		}
	}

	/** Passes one request to the web server, and its answer back, both streamed. */
	function forward(stream: ServerHttp2Stream, headers: Http2Headers): void {
		const request = http.request({
			host: target.host,
			port: target.port,
			agent,
			method: headers[':method'],
			path: headers[':path'],
			headers: { ...endToEnd(headers), host: headers[':authority'] }
		});
		let answered = false;
		request.on('response', (response) => {
			response.on('end', () => (answered = true));
			if (stream.destroyed) return void response.destroy();
			try {
				stream.respond({ ':status': response.statusCode ?? 502, ...endToEnd(response.headers) });
			} catch {
				response.destroy();
				return void stream.close(http2.constants.NGHTTP2_INTERNAL_ERROR);
			}
			response.pipe(stream);
		});
		// The relay shows its "didn't answer" page.
		request.on('error', () => {
			if (!stream.destroyed) stream.close(http2.constants.NGHTTP2_INTERNAL_ERROR);
		});
		stream.on('error', () => {});
		// The person went away before the answer finished (a closed tab ends its event streams).
		stream.on('close', () => {
			if (!answered) request.destroy();
		});
		stream.pipe(request);
	}

	function retryIn(ms: number): void {
		retry = setTimeout(attempt, ms);
	}

	function attempt(): void {
		if (stopped) return;
		const socket = new WebSocket(new URL(CONNECT_PATH, relay.server.replace(/^http/, 'ws')));
		let session: ServerHttp2Session | undefined;
		let ended = false;
		let pinging: NodeJS.Timeout | undefined;
		const waiting = setTimeout(() => end(), READY_TIMEOUT_MS);

		const stream = webSocketStream(socket, (text) => {
			let message: Partial<Ready>;
			try {
				message = JSON.parse(text) as Partial<Ready>;
			} catch {
				return;
			}
			if (message.type !== 'ready' || session) return;
			clearTimeout(waiting);
			const server = http2.createServer({
				settings: { initialWindowSize: STREAM_WINDOW_BYTES, maxConcurrentStreams: 256 },
				maxSessionMemory: 64
			});
			server.on('stream', forward);
			server.on('session', (created) => {
				session = created;
				created.on('error', () => {});
				created.on('close', () => end());
				created.once('connect', () => created.setLocalWindowSize(SESSION_WINDOW_BYTES));
			});
			server.emit('connection', stream);
			pinging = setInterval(ping, PING_MS);
			log(wasOnline ? 'connected to the relay again' : `nolune is reachable at ${message.url}`);
			wasOnline = true;
			failures = 0;
			reported = false;
		});

		/** A connection whose network went away never closes by itself: it stops answering. */
		function ping(): void {
			if (!session || session.destroyed) return;
			const timeout = setTimeout(() => end(), PING_TIMEOUT_MS);
			session.ping((err) => {
				clearTimeout(timeout);
				if (err) end();
			});
		}

		function end(code?: number, reason?: string): void {
			if (ended) return;
			ended = true;
			clearTimeout(waiting);
			clearInterval(pinging);
			const connected = session !== undefined;
			session?.destroy();
			stream.destroy();
			if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) {
				socket.close();
			}
			if (endAttempt === end) endAttempt = undefined;
			if (stopped) return;

			if (code === CLOSE.replaced) {
				return log(
					'another nolune connected to the relay with this address and took it over, so this one stopped using it. Only one computer can have an address.'
				);
			}
			if (code === CLOSE.released) {
				return log(
					'the relay address was given back, so nolune stopped using it. `nolune relay enable` gets a new one.'
				);
			}
			if (code === CLOSE.blocked) {
				return log(`the relay blocked this address (${reason}), so nolune stopped using it.`);
			}
			if (code === CLOSE.protocol) {
				return log(`the relay needs a newer nolune (${reason}): update nolune and restart it.`);
			}
			if (code === CLOSE.unknown) {
				// A config.json it can't read or write is tried again later, as a relay that can't be asked.
				void reclaim()
					.catch(() => 'later' as const)
					.then((outcome) => {
						if (stopped || outcome === 'stop') return;
						retryIn(outcome === 'back' ? 0 : UNKNOWN_RETRY_MS);
					});
				return;
			}
			if (connected) {
				log('lost the connection to the relay; reconnecting');
				return retryIn(1000);
			}
			failures++;
			if (!reported) log(`can't reach the relay at ${relay.server}; trying again`);
			reported = true;
			const backoff = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** failures);
			retryIn(backoff * (0.5 + Math.random() / 2));
		}

		endAttempt = end;
		socket.addEventListener('open', () => socket.send(JSON.stringify(hello)));
		socket.addEventListener('close', (event) => end(event.code, event.reason));
		// Node's WebSocket fires only 'error' when it can't connect. Once connected, a close with a
		// code of the relay's comes without one.
		socket.addEventListener('error', () => end());
	}

	attempt();
	return {
		close() {
			stopped = true;
			clearTimeout(retry);
			endAttempt?.();
			agent.destroy();
		}
	};
}
