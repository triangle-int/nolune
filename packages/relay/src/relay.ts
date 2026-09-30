import { randomBytes } from 'node:crypto';
import http, {
	type IncomingMessage,
	type OutgoingHttpHeaders,
	type ServerResponse
} from 'node:http';
import http2, { type ClientHttp2Session, type ClientHttp2Stream } from 'node:http2';
import { WebSocketServer, type WebSocket } from 'ws';
import { endToEnd } from './headers.ts';
import { isReservedName, randomName } from './names.ts';
import { page, type PageKind } from './pages.ts';
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
} from './protocol.ts';
import { GatewayStore } from './store.ts';
import { webSocketStream } from './stream.ts';

/*
 * The relay: gives a family's nolune a public HTTPS address without a tunnel or an open port on
 * their network (protocol.ts). It serves two kinds of host:
 *
 * - its own (`host`): registration and status (`/api/gateways`), and the WebSocket gateways keep
 *   open (`/api/connect`);
 * - `<name>.<domain>`: a family's nolune. Each request goes to that gateway as an HTTP/2 stream
 *   over its WebSocket, and the answer comes back the same way, streamed both ways, so uploads
 *   and event streams work as they do on the gateway itself.
 *
 * TLS is left to a proxy in front (Caddy, with a wildcard certificate for the domain). The relay
 * sees the requests it passes on, as any tunnel does, and keeps none of them: it logs only
 * registrations and gateways coming and going.
 */

export interface RelayOptions {
	/** Gateways get addresses under it: <name>.<domain>. */
	domain: string;
	/** The relay's own host, where gateways register and connect. Defaults to the domain. */
	host?: string;
	/** How the gateways' addresses start: https, unless testing without TLS. */
	scheme?: 'https' | 'http';
	/** Without one, gateways are kept in memory. */
	store?: GatewayStore;
	/**
	 * Behind a proxy that sets X-Forwarded-For (Caddy): a client's address is that header's last
	 * entry, rather than the connection's.
	 */
	trustProxy?: boolean;
	/** Registrations each client address may make in an hour. */
	registrationsPerHour?: number;
	/**
	 * For how long after a gateway leaves (restarting, or changing networks) requests wait for it
	 * to come back, rather than get the offline page at once.
	 */
	reconnectGraceMs?: number;
	log?: (message: string) => void;
}

export interface Relay {
	server: http.Server;
	/** Whether a gateway is connected now. */
	online(name: string): boolean;
	close(): Promise<void>;
}

interface Connection {
	name: string;
	socket: WebSocket;
	session: ClientHttp2Session;
}

const HELLO_TIMEOUT_MS = 10_000;
const HEARTBEAT_MS = 30_000;
const HOUR_MS = 60 * 60 * 1000;
/** Requests that may wait for one gateway to come back; more get the offline page at once. */
const MAX_WAITING = 64;
const MAX_API_BODY_BYTES = 4096;
/** Per stream and per connection, so a large download doesn't crawl over a long distance. */
const STREAM_WINDOW_BYTES = 1024 * 1024;
const SESSION_WINDOW_BYTES = 16 * 1024 * 1024;

/** What says who asked: only the relay's own word counts, never the client's. */
const FORWARDING = new Set(['forwarded', 'x-real-ip']);

function json(res: ServerResponse, status: number, body?: unknown): void {
	const text = body === undefined ? '' : JSON.stringify(body);
	res.writeHead(status, {
		'content-type': 'application/json',
		'content-length': Buffer.byteLength(text),
		'cache-control': 'no-store'
	});
	res.end(text);
}

function showPage(req: IncomingMessage, res: ServerResponse, status: number, kind: PageKind): void {
	const html = page(kind, req.headers['accept-language']);
	res.writeHead(status, {
		'content-type': 'text/html; charset=utf-8',
		'content-length': Buffer.byteLength(html),
		'cache-control': 'no-store',
		...(kind === 'offline' ? { 'retry-after': '20' } : {})
	});
	res.end(req.method === 'HEAD' ? undefined : html);
}

async function readJson(req: IncomingMessage): Promise<unknown> {
	let body = '';
	for await (const chunk of req) {
		body += chunk;
		if (body.length > MAX_API_BODY_BYTES) throw new Error('too large');
	}
	return body.trim() ? JSON.parse(body) : {};
}

function bearer(req: IncomingMessage): string {
	return /^Bearer\s+(\S+)$/i.exec(req.headers.authorization ?? '')?.[1] ?? '';
}

export function createRelay(options: RelayOptions): Relay {
	const domain = options.domain.toLowerCase();
	const apiHost = (options.host ?? domain).toLowerCase();
	const scheme = options.scheme ?? 'https';
	const store = options.store ?? new GatewayStore();
	const registrationsPerHour = options.registrationsPerHour ?? 10;
	const graceMs = options.reconnectGraceMs ?? 15_000;
	const log = options.log ?? (() => {});

	const connections = new Map<string, Connection>();
	/** When each gateway that isn't connected now last was. */
	const leftAt = new Map<string, number>();
	/** Requests waiting for a gateway to come back. */
	const waiting = new Map<string, Set<(connection: Connection | null) => void>>();
	/** Recent registrations by client address. */
	const registrations = new Map<string, number[]>();

	const urlOf = (name: string) => `${scheme}://${name}.${domain}`;

	/** The gateway a host is for, or null for the relay's own. */
	function gatewayName(host: string | undefined): string | null {
		const bare = (host ?? '').toLowerCase().replace(/:\d+$/, '').replace(/\.$/, '');
		if (bare === apiHost || !bare.endsWith(`.${domain}`)) return null;
		return bare.slice(0, -(domain.length + 1));
	}

	function clientAddress(req: IncomingMessage): string {
		if (options.trustProxy) {
			const forwarded = String(req.headers['x-forwarded-for'] ?? '')
				.split(',')
				.map((entry) => entry.trim())
				.filter(Boolean);
			if (forwarded.length) return forwarded[forwarded.length - 1];
		}
		return (req.socket.remoteAddress ?? '').replace(/^::ffff:/, '');
	}

	function mayRegister(address: string): boolean {
		const now = Date.now();
		const recent = (registrations.get(address) ?? []).filter((at) => now - at < HOUR_MS);
		const allowed = recent.length < registrationsPerHour;
		if (allowed) recent.push(now);
		registrations.set(address, recent);
		return allowed;
	}

	const pruning = setInterval(() => {
		const now = Date.now();
		for (const [address, times] of registrations) {
			if (times.every((at) => now - at >= HOUR_MS)) registrations.delete(address);
		}
		for (const [name, at] of leftAt) if (now - at >= graceMs) leftAt.delete(name);
	}, HOUR_MS);
	pruning.unref();

	// --- The relay's own API ---

	async function register(req: IncomingMessage, res: ServerResponse): Promise<void> {
		let body: { name?: unknown };
		try {
			body = (await readJson(req)) as { name?: unknown };
		} catch {
			return json(res, 400, { error: 'send JSON like {"name": "smiths"}, or nothing' });
		}
		if (!mayRegister(clientAddress(req))) {
			return json(res, 429, { error: 'too many new addresses from here; try again in an hour' });
		}
		let name: string;
		if (body?.name !== undefined && body.name !== '') {
			name = String(body.name).toLowerCase();
			if (!isValidName(name)) {
				return json(res, 400, {
					error:
						'a name has 3 to 32 lowercase letters, digits and single dashes, and starts and ends with a letter or digit'
				});
			}
			if (isReservedName(name) || store.has(name)) {
				return json(res, 409, { error: `${name} is taken; pick another name` });
			}
		} else {
			name = randomName();
			for (let tries = 0; store.has(name); tries++) {
				name = tries < 20 ? randomName() : `${randomName()}-${randomBytes(2).toString('hex')}`;
			}
		}
		const token = randomBytes(32).toString('base64url');
		store.add(name, token);
		log(`registered ${name}`);
		json(res, 201, { name, url: urlOf(name), token } satisfies Registration);
	}

	async function api(req: IncomingMessage, res: ServerResponse): Promise<void> {
		const { pathname } = new URL(req.url ?? '/', 'http://relay');
		if (pathname === GATEWAYS_PATH) {
			if (req.method === 'POST') return register(req, res);
			return json(res, 405, { error: 'POST to register a gateway' });
		}
		if (pathname.startsWith(`${GATEWAYS_PATH}/`)) {
			const name = decodeURIComponent(pathname.slice(GATEWAYS_PATH.length + 1));
			if (!store.verify(name, bearer(req))) {
				return json(res, 401, { error: 'no such gateway, or the wrong token' });
			}
			if (req.method === 'GET') {
				return json(res, 200, {
					name,
					url: urlOf(name),
					online: connections.has(name)
				} satisfies GatewayStatus);
			}
			if (req.method === 'DELETE') {
				store.remove(name);
				connections.get(name)?.socket.close(CLOSE.released, 'the name was given back');
				leftAt.delete(name);
				log(`released ${name}`);
				return json(res, 204);
			}
			return json(res, 405, { error: 'GET or DELETE' });
		}
		if (pathname === '/' && (req.method === 'GET' || req.method === 'HEAD')) {
			const text = 'nolune relay. See https://nolune.dev\n';
			res.writeHead(200, { 'content-type': 'text/plain; charset=utf-8' });
			return void res.end(req.method === 'HEAD' ? undefined : text);
		}
		json(res, 404, { error: 'not found' });
	}

	// --- Gateways' connections ---

	const wss = new WebSocketServer({ noServer: true, maxPayload: 32 * 1024 * 1024 });

	function attach(name: string, socket: WebSocket): void {
		connections.get(name)?.socket.close(CLOSE.replaced, 'another connection took over');
		const stream = webSocketStream(socket);
		socket.send(JSON.stringify({ type: 'ready', url: urlOf(name) } satisfies Ready));
		const session = http2.connect(`http://${name}.${domain}`, {
			createConnection: () => stream,
			settings: { enablePush: false, initialWindowSize: STREAM_WINDOW_BYTES },
			maxSessionMemory: 64
		});
		session.on('connect', () => session.setLocalWindowSize(SESSION_WINDOW_BYTES));
		session.on('error', (err) => log(`${name}: ${err.message}`));
		session.on('close', () => socket.close());
		const connection: Connection = { name, socket, session };
		connections.set(name, connection);
		leftAt.delete(name);
		store.seen(name);
		log(`${name} connected`);

		// A gateway whose network went away never says so: it stops answering pings.
		let alive = true;
		socket.on('pong', () => (alive = true));
		const heartbeat = setInterval(() => {
			if (!alive) return socket.terminate();
			alive = false;
			socket.ping();
		}, HEARTBEAT_MS);

		socket.on('close', () => {
			clearInterval(heartbeat);
			session.destroy();
			if (connections.get(name) !== connection) return;
			connections.delete(name);
			if (store.has(name)) leftAt.set(name, Date.now());
			log(`${name} disconnected`);
		});

		const waiters = waiting.get(name);
		waiting.delete(name);
		for (const resume of waiters ?? []) resume(connection);
	}

	wss.on('connection', (socket: WebSocket) => {
		// 'close' follows an error; without a listener, an error would stop the relay.
		socket.on('error', () => {});
		const timer = setTimeout(() => socket.close(CLOSE.noHello, 'no hello'), HELLO_TIMEOUT_MS);
		socket.once('message', (data, isBinary) => {
			clearTimeout(timer);
			let hello: Partial<Hello>;
			try {
				hello = isBinary ? {} : (JSON.parse(String(data)) as Partial<Hello>);
			} catch {
				hello = {};
			}
			if (hello.type !== 'hello') return socket.close(CLOSE.noHello, 'expected a hello');
			if (hello.protocol !== RELAY_PROTOCOL) {
				return socket.close(CLOSE.protocol, `this relay speaks protocol ${RELAY_PROTOCOL}`);
			}
			const name = String(hello.name ?? '');
			if (!store.verify(name, String(hello.token ?? ''))) {
				return socket.close(CLOSE.unknown, 'no such gateway, or the wrong token');
			}
			attach(name, socket);
		});
	});

	/**
	 * The gateway's connection. One that left a moment ago may be restarting: the request waits
	 * for it until the grace period after it left is over, and no longer.
	 */
	function connectionFor(name: string, req: IncomingMessage): Promise<Connection | null> {
		const connection = connections.get(name);
		if (connection) return Promise.resolve(connection);
		const remaining = (leftAt.get(name) ?? 0) + graceMs - Date.now();
		const waiters = waiting.get(name) ?? new Set();
		if (remaining <= 0 || waiters.size >= MAX_WAITING) return Promise.resolve(null);
		waiting.set(name, waiters);
		return new Promise((resolve) => {
			const done = (connection: Connection | null) => {
				clearTimeout(timer);
				req.off('close', gaveUp);
				waiters.delete(done);
				resolve(connection);
			};
			const gaveUp = () => done(null);
			const timer = setTimeout(gaveUp, remaining);
			req.once('close', gaveUp);
			waiters.add(done);
		});
	}

	/** Passes a request on to the gateway and its answer back. */
	function forward(req: IncomingMessage, res: ServerResponse, connection: Connection): void {
		const headers: OutgoingHttpHeaders = {
			...Object.fromEntries(
				Object.entries(endToEnd(req.headers)).filter(
					([name]) => !name.startsWith('x-forwarded-') && !FORWARDING.has(name)
				)
			),
			':method': req.method,
			':path': req.url,
			':scheme': scheme,
			':authority': req.headers.host,
			'x-forwarded-for': clientAddress(req),
			'x-forwarded-proto': scheme,
			'x-forwarded-host': req.headers.host
		};
		let upstream: ClientHttp2Stream;
		try {
			upstream = connection.session.request(headers);
		} catch (err) {
			log(`${connection.name}: ${(err as Error).message}`);
			return showPage(req, res, 502, 'failed');
		}
		upstream.on('response', (answer) => {
			res.writeHead(Number(answer[':status']), endToEnd(answer));
			upstream.pipe(res);
		});
		// The gateway couldn't answer, or stopped in the middle of an answer.
		upstream.on('error', () => {});
		upstream.on('close', () => {
			if (res.destroyed) return;
			if (!res.headersSent) showPage(req, res, 502, 'failed');
			else if (!res.writableEnded) res.destroy();
		});
		// The person went away (closed the tab, left the page): the gateway stops too.
		res.on('close', () => {
			if (!upstream.closed) upstream.close(http2.constants.NGHTTP2_CANCEL);
		});
		req.pipe(upstream);
	}

	async function gateway(name: string, req: IncomingMessage, res: ServerResponse): Promise<void> {
		const connection = await connectionFor(name, req);
		if (res.destroyed) return;
		if (connection) return forward(req, res, connection);
		if (store.has(name)) showPage(req, res, 503, 'offline');
		else showPage(req, res, 404, 'unknown');
	}

	const server = http.createServer(
		{
			// Uploads take as long as they take; headers still have to come quickly.
			requestTimeout: 0,
			headersTimeout: 60_000,
			// Longer than the proxy in front keeps idle connections (see Caddyfile).
			keepAliveTimeout: 75_000
		},
		(req, res) => {
			const name = gatewayName(req.headers.host);
			const handled = name === null ? api(req, res) : gateway(name, req, res);
			handled.catch((err: unknown) => {
				log(`request failed: ${(err as Error).message}`);
				if (!res.headersSent) json(res, 500, { error: 'the relay failed' });
				else res.destroy();
			});
		}
	);

	server.on('upgrade', (req, socket, head) => {
		const { pathname } = new URL(req.url ?? '/', 'http://relay');
		if (gatewayName(req.headers.host) === null && pathname === CONNECT_PATH) {
			wss.handleUpgrade(req, socket, head, (ws) => wss.emit('connection', ws, req));
			return;
		}
		// nolune's pages use event streams, never WebSockets: nothing to pass on.
		socket.end('HTTP/1.1 501 Not Implemented\r\nConnection: close\r\nContent-Length: 0\r\n\r\n');
	});

	return {
		server,
		online: (name) => connections.has(name),
		async close() {
			clearInterval(pruning);
			for (const waiters of waiting.values()) for (const resume of waiters) resume(null);
			for (const { socket } of connections.values()) socket.terminate();
			wss.close();
			await new Promise<void>((resolve) => {
				server.close(() => resolve());
				server.closeAllConnections();
			});
		}
	};
}
