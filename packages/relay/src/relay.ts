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
	PUSH,
	RELAY_PROTOCOL,
	isValidName,
	type GatewayStatus,
	type Hello,
	type PushResult,
	type Ready,
	type Registration
} from './protocol.ts';
import { MAX_PUSH_REQUEST_BYTES, readPush, type Apns } from './push.ts';
import { GatewayStore, currentMonth, network, type GatewayRecord } from './store.ts';
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
 *
 * It also passes the gateways' notifications on to Apple for the family's iPhones (push.ts), which
 * only take them from whoever holds the app's key.
 *
 * Anyone may register, so it keeps what one person can take in check: a limit on each address's
 * traffic in a month, on the addresses one network can register (in an hour, and in all), on the
 * notifications each address sends in an hour, and an operator who can block an address (`admin`,
 * served on a Unix socket, never on the web). Names don't stay taken for good: one whose nolune
 * hasn't connected in 90 days is free again (its nolune asks for it back if it returns, as long as
 * nobody else took it).
 */

export interface RelayOptions {
	/** Gateways get addresses under it: <name>.<domain>. */
	domain: string;
	/** The relay's own host, where gateways register and connect. Defaults to the domain. */
	host?: string;
	/**
	 * Where people who open the relay itself in a browser go: its host's `/`, and the domain
	 * without a name. Without one, the host's `/` says what it is in a line of text.
	 */
	site?: string;
	/** How the gateways' addresses start: https, unless testing without TLS. */
	scheme?: 'https' | 'http';
	/** Without one, gateways are kept in memory. */
	store?: GatewayStore;
	/**
	 * Behind a proxy that sets X-Forwarded-For (Caddy): a client's address is that header's last
	 * entry, rather than the connection's.
	 */
	trustProxy?: boolean;
	/** Registrations each network (network() of the client's address) may make in an hour. */
	registrationsPerHour?: number;
	/** Addresses each network may have registered at once; 0 for no limit. */
	maxGatewaysPerNetwork?: number;
	/** Bytes each address may pass through the relay in a month (UTC), both ways; 0 for no limit. */
	monthlyTrafficBytes?: number;
	/**
	 * How long a gateway may stay away before its name is free again: checked when the relay starts
	 * and every hour. Blocked ones are kept. 0 keeps every name.
	 */
	forgetAfterMs?: number;
	/**
	 * For how long after a gateway leaves (restarting, or changing networks) requests wait for it
	 * to come back, rather than get the offline page at once.
	 */
	reconnectGraceMs?: number;
	/**
	 * Apple's push service, with nolune for iOS's key (push.ts). Without it, gateways' notifications
	 * are refused. The relay closes it when it closes.
	 */
	apns?: Apns;
	/** Notifications each address may send in an hour, one an iPhone; 0 for no limit. */
	pushesPerHour?: number;
	log?: (message: string) => void;
}

export interface Relay {
	server: http.Server;
	/**
	 * The operator's API (admin.ts), for a Unix socket only they can open: the gateways, and
	 * blocking, unblocking and removing one.
	 */
	admin: http.Server;
	/** Whether a gateway is connected now. */
	online(name: string): boolean;
	close(): Promise<void>;
}

/** A gateway as the operator sees it (`GET /gateways` on the admin socket). */
export interface GatewayInfo {
	name: string;
	createdAt: string;
	lastSeenAt: string | null;
	online: boolean;
	trafficBytes: number;
	blocked: string | null;
}

interface Connection {
	name: string;
	socket: WebSocket;
	session: ClientHttp2Session;
}

const HELLO_TIMEOUT_MS = 10_000;
const HEARTBEAT_MS = 30_000;
const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;
/** Requests that may wait for one gateway to come back; more get the offline page at once. */
const MAX_WAITING = 64;
const MAX_API_BODY_BYTES = 4096;
const HEALTH_PATH = '/api/health';
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

/** Seconds until the next month starts (UTC), when traffic is counted afresh. */
function secondsToNextMonth(now = new Date()): number {
	const next = Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 1);
	return Math.ceil((next - now.getTime()) / 1000);
}

function showPage(req: IncomingMessage, res: ServerResponse, status: number, kind: PageKind): void {
	const html = page(kind, req.headers['accept-language']);
	const retryAfter =
		kind === 'offline' ? '20' : kind === 'quota' ? String(secondsToNextMonth()) : undefined;
	res.writeHead(status, {
		'content-type': 'text/html; charset=utf-8',
		'content-length': Buffer.byteLength(html),
		'cache-control': 'no-store',
		...(retryAfter ? { 'retry-after': retryAfter } : {})
	});
	res.end(req.method === 'HEAD' ? undefined : html);
}

/** A WebSocket close reason fits in 123 bytes. */
function closeReason(text: string): string {
	const characters = [...text];
	while (Buffer.byteLength(characters.join('')) > 123) characters.pop();
	return characters.join('');
}

async function readJson(req: IncomingMessage, limit = MAX_API_BODY_BYTES): Promise<unknown> {
	let body = '';
	for await (const chunk of req) {
		body += chunk;
		if (body.length > limit) throw new Error('too large');
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
	const maxPerNetwork = options.maxGatewaysPerNetwork ?? 10;
	const monthlyLimit = options.monthlyTrafficBytes ?? 30 * 1024 ** 3;
	const graceMs = options.reconnectGraceMs ?? 15_000;
	const forgetAfterMs = options.forgetAfterMs ?? 90 * DAY_MS;
	const pushesPerHour = options.pushesPerHour ?? 600;
	const log = options.log ?? (() => {});

	const connections = new Map<string, Connection>();
	/** When each gateway that isn't connected now last was. */
	const leftAt = new Map<string, number>();
	/** Requests waiting for a gateway to come back. */
	const waiting = new Map<string, Set<(connection: Connection | null) => void>>();
	/** Recent registrations by network. */
	const registrations = new Map<string, number[]>();
	/** The notifications each gateway sent this hour (hours since 1970). */
	const pushes = new Map<string, { hour: number; count: number }>();

	const urlOf = (name: string) => `${scheme}://${name}.${domain}`;

	function bareHost(host: string | undefined): string {
		return (host ?? '').toLowerCase().replace(/:\d+$/, '').replace(/\.$/, '');
	}

	/** The gateway a host is for, or null for the relay's own. */
	function gatewayName(host: string | undefined): string | null {
		const bare = bareHost(host);
		if (bare === apiHost || !bare.endsWith(`.${domain}`)) return null;
		return bare.slice(0, -(domain.length + 1));
	}

	/** Sends a browser on to the site. */
	function toSite(res: ServerResponse, site: string): void {
		res.writeHead(302, { location: site, 'cache-control': 'no-store' });
		res.end();
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

	function mayRegister(from: string): boolean {
		const now = Date.now();
		const recent = (registrations.get(from) ?? []).filter((at) => now - at < HOUR_MS);
		const allowed = recent.length < registrationsPerHour;
		if (allowed) recent.push(now);
		registrations.set(from, recent);
		return allowed;
	}

	/** Counts `count` notifications for a gateway, unless that would be more than it may send. */
	function mayPush(name: string, count: number): boolean {
		if (pushesPerHour <= 0) return true;
		const hour = Math.floor(Date.now() / HOUR_MS);
		const sent = pushes.get(name);
		const before = sent?.hour === hour ? sent.count : 0;
		if (before + count > pushesPerHour) return false;
		pushes.set(name, { hour, count: before + count });
		return true;
	}

	function overQuota(name: string): boolean {
		return monthlyLimit > 0 && store.trafficThisMonth(name) >= monthlyLimit;
	}

	/** Frees the names of gateways that have been away for longer than forgetAfterMs. */
	function forgetUnused(): void {
		if (forgetAfterMs <= 0) return;
		// One connected all along was last seen when it connected: now counts.
		for (const name of connections.keys()) store.seen(name);
		const forgotten = store.forgetUnseen(
			new Date(Date.now() - forgetAfterMs),
			(record) => connections.has(record.name) || record.blocked !== undefined
		);
		const days = Math.round(forgetAfterMs / DAY_MS);
		for (const name of forgotten) {
			leftAt.delete(name);
			log(`forgot ${name}: not connected in ${days} days`);
		}
	}

	forgetUnused();
	const pruning = setInterval(() => {
		const now = Date.now();
		for (const [address, times] of registrations) {
			if (times.every((at) => now - at >= HOUR_MS)) registrations.delete(address);
		}
		for (const [name, at] of leftAt) if (now - at >= graceMs) leftAt.delete(name);
		const hour = Math.floor(now / HOUR_MS);
		for (const [name, sent] of pushes) if (sent.hour !== hour) pushes.delete(name);
		forgetUnused();
	}, HOUR_MS);
	pruning.unref();
	// Traffic and when gateways were last seen change all the time: they're written once a minute.
	const flushing = setInterval(() => store.flush(), 60_000);
	flushing.unref();

	// --- The relay's own API ---

	async function register(req: IncomingMessage, res: ServerResponse): Promise<void> {
		let body: { name?: unknown };
		try {
			body = (await readJson(req)) as { name?: unknown };
		} catch {
			return json(res, 400, { error: 'send JSON like {"name": "smiths"}, or nothing' });
		}
		const from = network(clientAddress(req));
		if (!mayRegister(from)) {
			return json(res, 429, { error: 'too many new addresses from here; try again in an hour' });
		}
		if (maxPerNetwork > 0 && store.countFrom(from) >= maxPerNetwork) {
			return json(res, 429, {
				error: `this network already has ${maxPerNetwork} addresses, as many as one may; \`nolune relay disable\` on another computer gives one back`
			});
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
		store.add(name, token, from);
		log(`registered ${name}`);
		json(res, 201, { name, url: urlOf(name), token } satisfies Registration);
	}

	/** Sends a gateway's notification to the iPhones it names, through Apple. */
	async function push(name: string, req: IncomingMessage, res: ServerResponse): Promise<void> {
		if (req.method !== 'POST') return json(res, 405, { error: 'POST a notification' });
		const blocked = store.get(name)?.blocked;
		if (blocked) return json(res, 403, { error: blocked.reason });
		if (!options.apns) {
			return json(res, 501, { error: "this relay doesn't send notifications to iPhones" });
		}
		const message = await readJson(req, MAX_PUSH_REQUEST_BYTES).then(readPush, () => null);
		if (!message) {
			return json(res, 400, {
				error: 'send JSON like {"devices": [{"token": "<hex>"}], "title": "…", "body": "…"}'
			});
		}
		if (!mayPush(name, message.devices.length)) {
			return json(res, 429, {
				error: `this address sent ${pushesPerHour} notifications this hour, as many as it may`
			});
		}
		const { devices, ...notification } = message;
		const apns = options.apns;
		const outcomes = await Promise.all(
			devices.map((device) => apns.send(device, { ...notification, origin: urlOf(name) }))
		);
		json(res, 200, {
			sent: outcomes.filter((outcome) => outcome === 'sent').length,
			gone: devices.filter((_, i) => outcomes[i] === 'gone').map((device) => device.token)
		} satisfies PushResult);
	}

	async function api(req: IncomingMessage, res: ServerResponse): Promise<void> {
		const { pathname } = new URL(req.url ?? '/', 'http://relay');
		if (pathname === GATEWAYS_PATH) {
			if (req.method === 'POST') return register(req, res);
			return json(res, 405, { error: 'POST to register a gateway' });
		}
		if (pathname.startsWith(`${GATEWAYS_PATH}/`)) {
			const [encoded, action, ...rest] = pathname.slice(GATEWAYS_PATH.length + 1).split('/');
			const name = decodeURIComponent(encoded);
			if (rest.length || (action !== undefined && action !== PUSH)) {
				return json(res, 404, { error: 'not found' });
			}
			if (!store.verify(name, bearer(req))) {
				return json(res, 401, { error: 'no such gateway, or the wrong token' });
			}
			if (action === PUSH) return push(name, req, res);
			if (req.method === 'GET') {
				const blocked = store.get(name)?.blocked;
				return json(res, 200, {
					name,
					url: urlOf(name),
					online: connections.has(name),
					traffic: {
						month: currentMonth(),
						bytes: store.trafficThisMonth(name),
						limit: monthlyLimit > 0 ? monthlyLimit : null
					},
					...(blocked ? { blocked: blocked.reason } : {})
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
		const reading = req.method === 'GET' || req.method === 'HEAD';
		// For monitoring, and to check a relay that was just set up.
		if (pathname === HEALTH_PATH && reading) {
			res.writeHead(200, {
				'content-type': 'text/plain; charset=utf-8',
				'cache-control': 'no-store'
			});
			return void res.end(req.method === 'HEAD' ? undefined : 'ok\n');
		}
		if (pathname === '/' && reading) {
			if (options.site) return toSite(res, options.site);
			res.writeHead(200, { 'content-type': 'text/plain; charset=utf-8' });
			return void res.end(req.method === 'HEAD' ? undefined : 'nolune relay\n');
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
			if (store.has(name)) {
				leftAt.set(name, Date.now());
				store.seen(name);
			}
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
			const record = store.verify(name, String(hello.token ?? ''));
			if (!record) return socket.close(CLOSE.unknown, 'no such gateway, or the wrong token');
			if (record.blocked) return socket.close(CLOSE.blocked, closeReason(record.blocked.reason));
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
		const count = (chunk: Buffer) => store.addTraffic(connection.name, chunk.length);
		req.on('data', count);
		upstream.on('response', (answer) => {
			res.writeHead(Number(answer[':status']), endToEnd(answer));
			upstream.on('data', count);
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
		const record = store.get(name);
		if (record?.blocked) return showPage(req, res, 410, 'blocked');
		if (record && overQuota(name)) return showPage(req, res, 429, 'quota');
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
			// The domain without a name is nobody's nolune: whoever opens it goes to the site.
			if (options.site && domain !== apiHost && bareHost(req.headers.host) === domain) {
				return toSite(res, options.site);
			}
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

	// --- The operator's API ---

	function info(record: GatewayRecord): GatewayInfo {
		return {
			name: record.name,
			createdAt: record.createdAt,
			lastSeenAt: record.lastSeenAt ?? null,
			online: connections.has(record.name),
			trafficBytes: store.trafficThisMonth(record.name),
			blocked: record.blocked?.reason ?? null
		};
	}

	async function administer(req: IncomingMessage, res: ServerResponse): Promise<void> {
		const { pathname } = new URL(req.url ?? '/', 'http://relay');
		if (pathname === '/gateways' && req.method === 'GET') {
			return json(res, 200, store.list().map(info));
		}
		const match = /^\/gateways\/([^/]+)(?:\/(block|unblock))?$/.exec(pathname);
		const name = match ? decodeURIComponent(match[1]) : '';
		if (!match || !store.has(name)) return json(res, 404, { error: `no gateway ${name}` });
		const action = match[2];
		if (action === 'block' && req.method === 'POST') {
			const { reason } = (await readJson(req)) as { reason?: unknown };
			const why = String(reason ?? '').trim() || 'blocked by the relay';
			store.block(name, why);
			connections.get(name)?.socket.close(CLOSE.blocked, closeReason(why));
			log(`blocked ${name}: ${why}`);
		} else if (action === 'unblock' && req.method === 'POST') {
			store.unblock(name);
			log(`unblocked ${name}`);
		} else if (!action && req.method === 'DELETE') {
			store.remove(name);
			connections.get(name)?.socket.close(CLOSE.released, 'removed by the relay');
			leftAt.delete(name);
			log(`removed ${name}`);
			return json(res, 204);
		} else if (!action && req.method === 'GET') {
			// shown below
		} else {
			return json(res, 405, { error: 'not something the admin API does' });
		}
		json(res, 200, info(store.get(name)!));
	}

	const admin = http.createServer((req, res) => {
		administer(req, res).catch((err: unknown) => {
			if (!res.headersSent) json(res, 400, { error: (err as Error).message });
			else res.destroy();
		});
	});

	return {
		server,
		admin,
		online: (name) => connections.has(name),
		async close() {
			clearInterval(pruning);
			clearInterval(flushing);
			options.apns?.close();
			store.flush();
			for (const waiters of waiting.values()) for (const resume of waiters) resume(null);
			for (const { socket } of connections.values()) socket.terminate();
			wss.close();
			await new Promise<void>((resolve) => {
				server.close(() => resolve());
				server.closeAllConnections();
			});
			if (admin.listening) await new Promise((resolve) => admin.close(resolve));
		}
	};
}
