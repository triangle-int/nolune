import { mkdtempSync, readFileSync } from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { page, pageLanguage } from './pages.ts';
import { CLOSE, RELAY_PROTOCOL, isValidName, type Registration } from './protocol.ts';
import { createRelay, type Relay, type RelayOptions } from './relay.ts';
import { GatewayStore, hashToken } from './store.ts';

/*
 * The relay on its own: its API, what it shows for gateways that aren't there, and whom it lets
 * connect. packages/cli/src/relay.test.ts runs requests through it to a gateway.
 */

let relay: Relay | undefined;
let base = '';

async function startRelay(options: Partial<RelayOptions> = {}): Promise<Relay> {
	relay = createRelay({ domain: 'nolune.test', host: '127.0.0.1', scheme: 'http', ...options });
	await new Promise<void>((resolve) => relay!.server.listen(0, '127.0.0.1', resolve));
	base = `http://127.0.0.1:${(relay.server.address() as AddressInfo).port}`;
	return relay;
}

afterEach(async () => {
	await relay?.close();
	relay = undefined;
});

function register(body?: unknown): Promise<Response> {
	return fetch(`${base}/api/gateways`, {
		method: 'POST',
		body: body === undefined ? undefined : JSON.stringify(body)
	});
}

/** A request to a gateway's address, which only the Host header says. */
function visit(
	name: string,
	headers: Record<string, string> = {}
): Promise<{ status: number; headers: http.IncomingHttpHeaders; body: string }> {
	return new Promise((resolve, reject) => {
		const req = http.get(`${base}/`, { headers: { host: `${name}.nolune.test`, ...headers } });
		req.on('error', reject);
		req.on('response', async (res) => {
			let body = '';
			for await (const chunk of res) body += chunk;
			resolve({ status: res.statusCode!, headers: res.headers, body });
		});
	});
}

/** Connects as a gateway would and resolves with how the relay closed or what it answered. */
function hello(message: unknown): Promise<{ code?: number; ready?: unknown }> {
	return new Promise((resolve) => {
		const socket = new WebSocket(`${base.replace('http', 'ws')}/api/connect`);
		socket.addEventListener('open', () => socket.send(JSON.stringify(message)));
		socket.addEventListener('message', (event) => {
			resolve({ ready: JSON.parse(String(event.data)) });
			socket.close();
		});
		socket.addEventListener('close', (event) => resolve({ code: event.code }));
	});
}

describe('registration', () => {
	it('gives a random name with its address and a token', async () => {
		await startRelay();
		const res = await register();
		expect(res.status).toBe(201);
		const { name, url, token } = (await res.json()) as Registration;
		expect(isValidName(name)).toBe(true);
		expect(url).toBe(`http://${name}.nolune.test`);
		expect(token.length).toBeGreaterThanOrEqual(40);
	});

	it('gives the name asked for, once', async () => {
		await startRelay();
		const first = await register({ name: 'Smiths' });
		expect(((await first.json()) as Registration).name).toBe('smiths');
		const again = await register({ name: 'smiths' });
		expect(again.status).toBe(409);
		expect(((await again.json()) as { error: string }).error).toMatch(/taken/);
	});

	it("refuses names that aren't a single label, or that belong to the site", async () => {
		await startRelay();
		for (const name of ['ab', 'a.b.c', '-smiths', 'xn--80ak6aa92e', 'smiths_', 'x'.repeat(33)]) {
			expect((await register({ name })).status).toBe(400);
		}
		expect((await register({ name: 'www' })).status).toBe(409);
		expect((await register({ name: 'relay' })).status).toBe(409);
	});

	it('limits how many addresses one client gets in an hour', async () => {
		await startRelay({ registrationsPerHour: 2 });
		expect((await register()).status).toBe(201);
		expect((await register()).status).toBe(201);
		expect((await register()).status).toBe(429);
	});

	it('tells a gateway its status, and gives its name back, only with its token', async () => {
		await startRelay();
		const { name, token } = (await (await register({ name: 'smiths' })).json()) as Registration;
		const url = `${base}/api/gateways/${name}`;
		expect((await fetch(url)).status).toBe(401);
		expect((await fetch(url, { headers: { authorization: 'Bearer nope' } })).status).toBe(401);

		const auth = { authorization: `Bearer ${token}` };
		expect(await (await fetch(url, { headers: auth })).json()).toEqual({
			name: 'smiths',
			url: 'http://smiths.nolune.test',
			online: false
		});
		expect((await fetch(url, { method: 'DELETE', headers: auth })).status).toBe(204);
		expect((await fetch(url, { headers: auth })).status).toBe(401);
		// The name is free again.
		expect((await register({ name: 'smiths' })).status).toBe(201);
	});
});

describe('addresses without a gateway', () => {
	it("shows an offline page, in the browser's language, for a gateway that isn't connected", async () => {
		await startRelay();
		await register({ name: 'smiths' });
		const res = await visit('smiths', { 'accept-language': 'ru-RU,ru;q=0.9,en;q=0.8' });
		expect(res.status).toBe(503);
		expect(res.headers['retry-after']).toBe('20');
		expect(res.body).toContain('nolune сейчас не на связи');
		expect(res.body).toContain('http-equiv="refresh"');
	});

	it('shows that nothing is at an address nobody registered', async () => {
		await startRelay();
		const res = await visit('nobody');
		expect(res.status).toBe(404);
		expect(res.body).toContain('No nolune here');
	});

	it('picks the browser language it has', () => {
		expect(pageLanguage('de-CH, fr;q=0.9')).toBe('de');
		expect(pageLanguage('ja, fr;q=0.5')).toBe('fr');
		expect(pageLanguage('es;q=0.2, ru;q=0.8')).toBe('ru');
		expect(pageLanguage('ja')).toBe('en');
		expect(pageLanguage(undefined)).toBe('en');
		expect(page('failed', 'es')).toContain('nolune no respondió');
	});
});

describe('connections', () => {
	it('lets a gateway in with its token, and says where it is', async () => {
		const running = await startRelay();
		const { name, token } = (await (await register({ name: 'smiths' })).json()) as Registration;
		const answer = await hello({ type: 'hello', protocol: RELAY_PROTOCOL, name, token });
		expect(answer.ready).toEqual({ type: 'ready', url: 'http://smiths.nolune.test' });
		expect(running.online('smiths')).toBe(true);
	});

	it('closes connections with a wrong token, another protocol, or no hello', async () => {
		await startRelay();
		const { name, token } = (await (await register({ name: 'smiths' })).json()) as Registration;
		expect(await hello({ type: 'hello', protocol: RELAY_PROTOCOL, name, token: 'nope' })).toEqual({
			code: CLOSE.unknown
		});
		expect(await hello({ type: 'hello', protocol: RELAY_PROTOCOL, name: 'nobody', token })).toEqual(
			{ code: CLOSE.unknown }
		);
		expect(await hello({ type: 'hello', protocol: RELAY_PROTOCOL + 1, name, token })).toEqual({
			code: CLOSE.protocol
		});
		expect(await hello({ type: 'hi' })).toEqual({ code: CLOSE.noHello });
	});

	it("doesn't pass WebSockets on to gateways", async () => {
		await startRelay();
		await register({ name: 'smiths' });
		const status = await new Promise<number>((resolve) => {
			const req = http.get(`${base}/`, {
				headers: {
					host: 'smiths.nolune.test',
					connection: 'upgrade',
					upgrade: 'websocket',
					'sec-websocket-version': '13',
					'sec-websocket-key': 'dGhlIHNhbXBsZSBub25jZQ=='
				}
			});
			req.on('response', (res) => resolve(res.statusCode!));
			req.on('upgrade', () => resolve(101));
		});
		expect(status).toBe(501);
	});
});

describe('the gateways file', () => {
	it('keeps gateways across restarts, with a hash of the token rather than the token', () => {
		const file = join(mkdtempSync(join(tmpdir(), 'nolune-relay-')), 'gateways.json');
		new GatewayStore(file).add('smiths', 'secret-token');
		const saved = readFileSync(file, 'utf8');
		expect(saved).not.toContain('secret-token');
		expect(saved).toContain(hashToken('secret-token'));

		const reopened = new GatewayStore(file);
		expect(reopened.verify('smiths', 'secret-token')?.name).toBe('smiths');
		expect(reopened.verify('smiths', 'other')).toBeNull();
		reopened.remove('smiths');
		expect(new GatewayStore(file).has('smiths')).toBe(false);
	});
});
