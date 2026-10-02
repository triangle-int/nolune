import { generateKeyPairSync, verify } from 'node:crypto';
import http2, { type IncomingHttpHeaders } from 'node:http2';
import type { AddressInfo } from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import type { PushResult } from './protocol.ts';
import { cut, createApns, payload, readPush, type Apns, type ApnsOutcome } from './push.ts';
import { createRelay, type Relay } from './relay.ts';
import { GatewayStore } from './store.ts';

/*
 * Notifications for the family's iPhones: what the relay takes from a gateway, what it sends
 * Apple, and how it reads Apple's answers (from a stand-in for Apple's servers).
 */

const TOKEN = 'a1'.repeat(32);
const OTHER = 'b2'.repeat(32);

const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
const KEY = privateKey.export({ type: 'pkcs8', format: 'pem' }).toString();

/** Closed last to first: Apple's stand-in waits for the connections to it. */
const closing: (() => unknown)[] = [];
afterEach(async () => {
	for (const close of closing.splice(0).reverse()) await close();
});

describe("a gateway's notification", () => {
	it('names iPhones by their tokens, and has a title and a body', () => {
		expect(
			readPush({ devices: [{ token: TOKEN }], title: 'Umbrellas', body: 'Rain at 3pm.' })
		).toEqual({
			devices: [{ token: TOKEN, sandbox: false }],
			title: 'Umbrellas',
			body: 'Rain at 3pm.'
		});
		expect(
			readPush({
				devices: [{ token: TOKEN, sandbox: true }],
				title: 'Umbrellas',
				subtitle: 'Family',
				body: '',
				thread: 'family',
				path: '/p/family?notification=1'
			})
		).toMatchObject({
			devices: [{ sandbox: true }],
			subtitle: 'Family',
			path: '/p/family?notification=1'
		});
	});

	it("isn't one without iPhones, text, or with a way off the family's address", () => {
		const good = { devices: [{ token: TOKEN }], title: 'Umbrellas', body: 'Rain.' };
		for (const bad of [
			null,
			'text',
			{ ...good, devices: [] },
			{ ...good, devices: Array.from({ length: 101 }, () => ({ token: TOKEN })) },
			{ ...good, devices: [{ token: 'not hex' }] },
			{ ...good, devices: [{ token: 'ab' }] },
			{ ...good, title: undefined },
			{ ...good, body: 3 },
			{ ...good, path: 'https://example.com' },
			{ ...good, path: '//example.com' },
			{ ...good, path: '/\\example.com' }
		]) {
			expect(readPush(bad)).toBeNull();
		}
	});
});

describe('what Apple gets', () => {
	it('is an alert with a sound, grouped by profile, and the path to open', () => {
		expect(
			JSON.parse(
				payload({
					title: 'Umbrellas',
					subtitle: 'Family',
					body: 'Rain.',
					thread: 'family',
					path: '/p/family',
					origin: 'https://smiths.nolune.family'
				})
			)
		).toEqual({
			aps: {
				alert: { title: 'Umbrellas', subtitle: 'Family', body: 'Rain.' },
				sound: 'default',
				'thread-id': 'family'
			},
			path: '/p/family',
			origin: 'https://smiths.nolune.family'
		});
	});

	it('fits in 4 KB, however long the text and in whatever script', () => {
		for (const body of [
			'a'.repeat(10_000),
			'дождь '.repeat(2_000),
			'"\n'.repeat(5_000),
			'🌧'.repeat(3_000)
		]) {
			const json = payload({ title: 'T'.repeat(1_000), body });
			expect(Buffer.byteLength(json)).toBeLessThanOrEqual(4096);
			const { alert } = (JSON.parse(json) as { aps: { alert: { title: string; body: string } } })
				.aps;
			expect(alert.body.endsWith('…')).toBe(true);
			expect(alert.body.length).toBeGreaterThan(500);
			expect(Buffer.byteLength(alert.title)).toBeLessThanOrEqual(256);
		}
		expect(cut('short', 100)).toBe('short');
		expect(cut('дождь', 5)).toBe('д…');
	});
});

/** A stand-in for Apple's servers, answering each request with `answer(token)`. */
async function fakeApple(answer: (token: string) => { status: number; reason?: string }) {
	const requests: { headers: IncomingHttpHeaders; body: unknown }[] = [];
	const server = http2.createServer();
	server.on('stream', (stream, headers) => {
		let body = '';
		stream.setEncoding('utf8');
		stream.on('data', (chunk: string) => (body += chunk));
		stream.on('end', () => {
			requests.push({ headers, body: JSON.parse(body) });
			const { status, reason } = answer(String(headers[':path']).split('/').pop()!);
			stream.respond({ ':status': status });
			stream.end(reason ? JSON.stringify({ reason }) : undefined);
		});
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	closing.push(() => new Promise((resolve) => server.close(resolve)));
	return { url: `http://127.0.0.1:${(server.address() as AddressInfo).port}`, requests };
}

describe("Apple's push service", () => {
	it('gets each notification with a token signed with the key', async () => {
		const apple = await fakeApple(() => ({ status: 200 }));
		const sandbox = await fakeApple(() => ({ status: 200 }));
		const apns = createApns({
			key: Buffer.from(KEY).toString('base64'),
			keyId: 'KEY123',
			teamId: 'TEAM456',
			topic: 'dev.nolune.app',
			servers: { production: apple.url, sandbox: sandbox.url }
		});
		closing.push(() => apns.close());

		expect(await apns.send({ token: TOKEN }, { title: 'Umbrellas', body: 'Rain.' })).toBe('sent');
		expect(
			await apns.send({ token: OTHER, sandbox: true }, { title: 'Umbrellas', body: 'Rain.' })
		).toBe('sent');
		expect(apple.requests).toHaveLength(1);
		expect(sandbox.requests).toHaveLength(1);

		const { headers, body } = apple.requests[0];
		expect(headers).toMatchObject({
			':method': 'POST',
			':path': `/3/device/${TOKEN}`,
			'apns-topic': 'dev.nolune.app',
			'apns-push-type': 'alert',
			'apns-priority': '10'
		});
		expect(body).toMatchObject({ aps: { alert: { title: 'Umbrellas', body: 'Rain.' } } });

		const [head, claims, signature] = String(headers.authorization)
			.replace(/^bearer /, '')
			.split('.');
		expect(JSON.parse(Buffer.from(head, 'base64url').toString())).toEqual({
			alg: 'ES256',
			kid: 'KEY123'
		});
		expect(JSON.parse(Buffer.from(claims, 'base64url').toString())).toMatchObject({
			iss: 'TEAM456'
		});
		expect(
			verify(
				'sha256',
				Buffer.from(`${head}.${claims}`),
				{ key: publicKey, dsaEncoding: 'ieee-p1363' },
				Buffer.from(signature, 'base64url')
			)
		).toBe(true);
	});

	it("tells iPhones that are gone from ones it couldn't reach", async () => {
		const answers: Record<string, { status: number; reason?: string }> = {
			['1'.repeat(64)]: { status: 410, reason: 'Unregistered' },
			['2'.repeat(64)]: { status: 400, reason: 'BadDeviceToken' },
			['3'.repeat(64)]: { status: 429, reason: 'TooManyRequests' },
			['4'.repeat(64)]: { status: 500, reason: 'InternalServerError' }
		};
		const apple = await fakeApple((token) => answers[token]);
		const logged: string[] = [];
		const apns = createApns({
			key: KEY,
			keyId: 'KEY123',
			teamId: 'TEAM456',
			topic: 'dev.nolune.app',
			servers: { production: apple.url, sandbox: apple.url },
			log: (message) => logged.push(message)
		});
		closing.push(() => apns.close());
		const outcomes = await Promise.all(
			Object.keys(answers).map((token) => apns.send({ token }, { title: 'T', body: 'B' }))
		);
		expect(outcomes).toEqual(['gone', 'gone', 'failed', 'failed']);
		expect(logged).toEqual([
			"Apple didn't take a notification: 429 TooManyRequests",
			"Apple didn't take a notification: 500 InternalServerError"
		]);
	});
});

describe('the relay', () => {
	let relay: Relay | undefined;
	let base = '';
	afterEach(async () => {
		await relay?.close();
		relay = undefined;
	});

	/** A relay with one gateway, smiths, and Apple answering with `outcome` for every iPhone. */
	async function start(
		options: {
			apns?: boolean;
			pushesPerHour?: number;
			outcome?: (token: string) => ApnsOutcome;
		} = {}
	) {
		const sent: { token: string; title: string; origin?: string }[] = [];
		const apns: Apns = {
			async send(device, notification) {
				sent.push({ token: device.token, title: notification.title, origin: notification.origin });
				return options.outcome?.(device.token) ?? 'sent';
			},
			close() {}
		};
		const store = new GatewayStore();
		store.add('smiths', 'secret');
		relay = createRelay({
			domain: 'nolune.test',
			host: '127.0.0.1',
			scheme: 'http',
			store,
			apns: options.apns === false ? undefined : apns,
			pushesPerHour: options.pushesPerHour
		});
		await new Promise<void>((resolve) => relay!.server.listen(0, '127.0.0.1', resolve));
		base = `http://127.0.0.1:${(relay.server.address() as AddressInfo).port}`;
		return { sent, store };
	}

	function push(body: unknown, token = 'secret'): Promise<Response> {
		return fetch(`${base}/api/gateways/smiths/push`, {
			method: 'POST',
			headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
			body: JSON.stringify(body)
		});
	}

	const notification = (...tokens: string[]) => ({
		devices: tokens.map((token) => ({ token })),
		title: 'Umbrellas',
		body: 'Rain at 3pm.'
	});

	it("sends a gateway's notification to its iPhones and says which are gone", async () => {
		const { sent } = await start({ outcome: (token) => (token === OTHER ? 'gone' : 'sent') });
		const res = await push(notification(TOKEN, OTHER));
		expect(res.status).toBe(200);
		expect((await res.json()) as PushResult).toEqual({ sent: 1, gone: [OTHER] });
		// With the family's address, for the app to check it has that nolune open.
		expect(sent).toEqual([
			{ token: TOKEN, title: 'Umbrellas', origin: 'http://smiths.nolune.test' },
			{ token: OTHER, title: 'Umbrellas', origin: 'http://smiths.nolune.test' }
		]);
	});

	it('takes them only from the gateway, and only ones it can read', async () => {
		const { sent } = await start();
		expect((await push(notification(TOKEN), 'wrong')).status).toBe(401);
		expect((await push({ devices: [], title: 'x', body: 'y' })).status).toBe(400);
		const unknown = await fetch(`${base}/api/gateways/smiths/pull`, {
			method: 'POST',
			headers: { authorization: 'Bearer secret' }
		});
		expect(unknown.status).toBe(404);
		expect(sent).toEqual([]);
	});

	it('counts each iPhone toward what an address may send in an hour', async () => {
		const { sent } = await start({ pushesPerHour: 3 });
		expect((await push(notification(TOKEN, OTHER))).status).toBe(200);
		const over = await push(notification(TOKEN, OTHER));
		expect(over.status).toBe(429);
		expect(((await over.json()) as { error: string }).error).toMatch(/3 notifications this hour/);
		expect((await push(notification(TOKEN))).status).toBe(200);
		expect(sent).toHaveLength(3);
	});

	it("says so when it can't send to iPhones, or the address is blocked", async () => {
		await start({ apns: false });
		const res = await push(notification(TOKEN));
		expect(res.status).toBe(501);
		await relay!.close();

		const { store } = await start();
		store.block('smiths', 'spam');
		const blocked = await push(notification(TOKEN));
		expect(blocked.status).toBe(403);
		expect(((await blocked.json()) as { error: string }).error).toBe('spam');
	});
});
