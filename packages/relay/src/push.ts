import { createPrivateKey, sign, type KeyObject } from 'node:crypto';
import http2, { type ClientHttp2Session, type ClientHttp2Stream } from 'node:http2';
import type { Push } from './protocol.ts';

/*
 * Notifications on the family's iPhones (nolune for iOS, ios/ in the repository). Apple's push
 * service (APNs) only takes notifications for an app from whoever holds the app's key, which only
 * nolune's publisher can have, so it stays on the relay and gateways send through it
 * (`POST /api/gateways/<name>/push`, protocol.ts). The relay passes each one on and keeps none.
 *
 * Each request to Apple carries a token signed with the key (a JWT), made again every 50 minutes:
 * Apple takes one for an hour and refuses new ones more often than every 20.
 */

export interface ApnsOptions {
	/** The key from Apple (AuthKey_<id>.p8): its PEM, or the PEM in base64 on one line. */
	key: string;
	keyId: string;
	teamId: string;
	/** The app's bundle ID. */
	topic: string;
	/** Where to send. Apple's servers by default; tests use their own. */
	servers?: { production: string; sandbox: string };
	log?: (message: string) => void;
}

/** Apple took it; the token is no app's any more (it was deleted, say); or it didn't go. */
export type ApnsOutcome = 'sent' | 'gone' | 'failed';

/** A gateway's notification, and its address (`origin`), which the relay adds. */
export type ApnsNotification = Omit<Push, 'devices'> & { origin?: string };

export interface Apns {
	send(
		device: { token: string; sandbox?: boolean },
		notification: ApnsNotification
	): Promise<ApnsOutcome>;
	close(): void;
}

const SERVERS = {
	production: 'https://api.push.apple.com',
	sandbox: 'https://api.sandbox.push.apple.com'
};

const TOKEN_LIFETIME_MS = 50 * 60 * 1000;
const REQUEST_TIMEOUT_MS = 15_000;
/** While an iPhone is off, Apple keeps its newest notification for a day, then drops it. */
const KEEP_FOR_S = 24 * 60 * 60;
/** The most a notification's payload may have, in bytes. */
const MAX_PAYLOAD_BYTES = 4096;

/** Apple's reasons for a token that's no device's for this app. */
const GONE = new Set(['BadDeviceToken', 'DeviceTokenNotForTopic', 'Unregistered']);

/** At most this many iPhones a notification, and this much of the request. */
export const MAX_PUSH_DEVICES = 100;
export const MAX_PUSH_REQUEST_BYTES = 32 * 1024;

const DEVICE_TOKEN = /^[0-9a-f]{32,200}$/i;

/** A gateway's request, checked: null when it isn't one. */
export function readPush(body: unknown): Push | null {
	if (!body || typeof body !== 'object') return null;
	const { devices, title, subtitle, body: text, thread, path } = body as Record<string, unknown>;
	if (!Array.isArray(devices) || devices.length === 0 || devices.length > MAX_PUSH_DEVICES) {
		return null;
	}
	const parsed: Push['devices'] = [];
	for (const device of devices as unknown[]) {
		const { token, sandbox } = (device ?? {}) as Record<string, unknown>;
		if (typeof token !== 'string' || !DEVICE_TOKEN.test(token)) return null;
		parsed.push({ token, sandbox: sandbox === true });
	}
	const optional = (value: unknown) => value === undefined || typeof value === 'string';
	if (typeof title !== 'string' || typeof text !== 'string') return null;
	if (!optional(subtitle) || !optional(thread) || !optional(path)) return null;
	// A path on the family's own address, never a way off it.
	if (typeof path === 'string' && (!path.startsWith('/') || /^\/[/\\]/.test(path))) return null;
	return {
		devices: parsed,
		title,
		body: text,
		...(subtitle ? { subtitle } : {}),
		...(thread ? { thread } : {}),
		...(path ? { path } : {})
	};
}

/** `text` cut to at most `bytes` of UTF-8, with an ellipsis where it was cut. */
export function cut(text: string, bytes: number): string {
	if (Buffer.byteLength(text) <= bytes) return text;
	const characters = [...text];
	let keep = Math.floor((characters.length * bytes) / Buffer.byteLength(text));
	while (keep > 0 && Buffer.byteLength(characters.slice(0, keep).join('') + '…') > bytes) keep--;
	return characters.slice(0, keep).join('').trimEnd() + '…';
}

/**
 * What Apple gets: the alert, and for the app (outside `aps`) the path to open on the family's
 * address, which it checks is the one it has open. The title and subtitle are a line each; the
 * body takes what room is left.
 */
export function payload(notification: ApnsNotification): string {
	const make = (body: string) =>
		JSON.stringify({
			aps: {
				alert: {
					title: cut(notification.title, 256),
					...(notification.subtitle ? { subtitle: cut(notification.subtitle, 256) } : {}),
					body
				},
				sound: 'default',
				...(notification.thread ? { 'thread-id': cut(notification.thread, 64) } : {})
			},
			...(notification.path ? { path: notification.path } : {}),
			...(notification.origin ? { origin: notification.origin } : {})
		});
	const room = MAX_PAYLOAD_BYTES - Buffer.byteLength(make(''));
	// JSON escapes some characters (quotes, newlines) into two or more: cut until it fits.
	let body = notification.body;
	for (let budget = room; budget > 0; budget -= 64) {
		const json = make(body);
		if (Buffer.byteLength(json) <= MAX_PAYLOAD_BYTES) return json;
		body = cut(notification.body, budget);
	}
	return make('');
}

/** The key Apple gave, as Node reads it. Throws when it isn't one. */
export function privateKey(key: string): KeyObject {
	const pem = key.includes('BEGIN') ? key : Buffer.from(key, 'base64').toString('utf8');
	return createPrivateKey(pem);
}

function base64url(value: unknown): string {
	return Buffer.from(JSON.stringify(value)).toString('base64url');
}

export function createApns(options: ApnsOptions): Apns {
	const key = privateKey(options.key);
	const servers = options.servers ?? SERVERS;
	const log = options.log ?? (() => {});
	const sessions = new Map<string, ClientHttp2Session>();
	let jwt: { value: string; at: number } | undefined;

	function authorization(): string {
		const now = Date.now();
		if (!jwt || now - jwt.at > TOKEN_LIFETIME_MS) {
			const unsigned = `${base64url({ alg: 'ES256', kid: options.keyId })}.${base64url({
				iss: options.teamId,
				iat: Math.floor(now / 1000)
			})}`;
			const signature = sign('sha256', Buffer.from(unsigned), { key, dsaEncoding: 'ieee-p1363' });
			jwt = { value: `${unsigned}.${signature.toString('base64url')}`, at: now };
		}
		return `bearer ${jwt.value}`;
	}

	/** One connection to each of Apple's servers, kept open for the next notification. */
	function session(server: string): ClientHttp2Session {
		const open = sessions.get(server);
		if (open && !open.closed && !open.destroyed) return open;
		const created = http2.connect(server);
		const forget = () => {
			if (sessions.get(server) === created) sessions.delete(server);
		};
		created.on('error', (err) => {
			forget();
			log(`Apple's push service: ${err.message}`);
		});
		created.on('goaway', forget);
		created.on('close', forget);
		sessions.set(server, created);
		return created;
	}

	function send(
		device: { token: string; sandbox?: boolean },
		notification: ApnsNotification
	): Promise<ApnsOutcome> {
		return new Promise((resolve) => {
			let stream: ClientHttp2Stream;
			try {
				stream = session(device.sandbox ? servers.sandbox : servers.production).request({
					':method': 'POST',
					':path': `/3/device/${device.token}`,
					authorization: authorization(),
					'apns-topic': options.topic,
					'apns-push-type': 'alert',
					'apns-priority': '10',
					'apns-expiration': String(Math.floor(Date.now() / 1000) + KEEP_FOR_S),
					'content-type': 'application/json'
				});
			} catch (err) {
				log(`Apple's push service: ${(err as Error).message}`);
				return resolve('failed');
			}
			let status = 0;
			let answer = '';
			stream.setEncoding('utf8');
			stream.setTimeout(REQUEST_TIMEOUT_MS, () => stream.close(http2.constants.NGHTTP2_CANCEL));
			stream.on('response', (headers) => (status = Number(headers[':status'])));
			stream.on('data', (chunk: string) => (answer += chunk));
			// 'close' follows, with no status.
			stream.on('error', () => {});
			stream.on('close', () => {
				if (status === 200) return resolve('sent');
				let reason = '';
				try {
					reason = String((JSON.parse(answer) as { reason?: unknown }).reason ?? '');
				} catch {
					// no body, or not JSON
				}
				if (status === 410 || GONE.has(reason)) return resolve('gone');
				// The signed token was refused: the next request makes a new one.
				if (status === 403) jwt = undefined;
				log(`Apple didn't take a notification: ${status || 'no answer'} ${reason}`.trim());
				resolve('failed');
			});
			stream.end(payload(notification));
		});
	}

	return {
		send,
		close() {
			for (const open of sessions.values()) open.close();
			sessions.clear();
		}
	};
}
