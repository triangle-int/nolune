import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { initConfig, readConfig, webhookUrl } from '@nolune/core';
import type { Registration } from '@nolune/relay/protocol';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRelay, type Relay } from '../../relay/src/relay.ts';
import { connectRelay, type RelayLink } from './relay.ts';
import { runCli } from './run.ts';
import { testIo } from './test/io.ts';

/*
 * A browser's request through the relay to a gateway and its web server, and back: the relay
 * (packages/relay) on one side, connectRelay() on the other, and a stand-in for the gateway's web
 * server behind it.
 */

let relay: Relay;
let relayUrl = '';
let web: http.Server;
let webPort = 0;
const links: RelayLink[] = [];
/** What the web server was asked, and whether each request's connection is still open. */
let received: { req: http.IncomingMessage; body: Buffer; open: boolean }[] = [];
let respond: (req: http.IncomingMessage, res: http.ServerResponse, body: Buffer) => void;

beforeEach(async () => {
	initConfig();
	relay = createRelay({
		domain: 'nolune.test',
		host: '127.0.0.1',
		scheme: 'http',
		reconnectGraceMs: 2000
	});
	await new Promise<void>((resolve) => relay.server.listen(0, '127.0.0.1', resolve));
	relayUrl = `http://127.0.0.1:${(relay.server.address() as AddressInfo).port}`;

	received = [];
	respond = (_req, res) => res.end('hello');
	web = http.createServer(async (req, res) => {
		const entry = { req, body: Buffer.alloc(0), open: true };
		received.push(entry);
		res.on('close', () => (entry.open = false));
		const chunks: Buffer[] = [];
		for await (const chunk of req) chunks.push(chunk);
		entry.body = Buffer.concat(chunks);
		respond(req, res, entry.body);
	});
	await new Promise<void>((resolve) => web.listen(0, '127.0.0.1', resolve));
	webPort = (web.address() as AddressInfo).port;
});

afterEach(async () => {
	for (const link of links.splice(0)) link.close();
	await relay.close();
	web.closeAllConnections();
	await new Promise((resolve) => web.close(resolve));
	vi.restoreAllMocks();
});

async function register(name: string): Promise<Registration> {
	const res = await fetch(`${relayUrl}/api/gateways`, {
		method: 'POST',
		body: JSON.stringify({ name })
	});
	return (await res.json()) as Registration;
}

/** Connects a gateway and resolves once the relay has it, with what it logged. */
async function connect(registration: Registration): Promise<{ link: RelayLink; log: string[] }> {
	const log: string[] = [];
	const link = connectRelay(
		{ server: relayUrl, ...registration },
		{ host: '127.0.0.1', port: webPort },
		(message) => log.push(message)
	);
	links.push(link);
	await vi.waitFor(() => expect(relay.online(registration.name)).toBe(true));
	return { link, log };
}

interface Answer {
	status: number;
	headers: http.IncomingHttpHeaders;
	body: Buffer;
}

/** A browser's request to the gateway's address. */
function request(
	path: string,
	opts: { method?: string; headers?: Record<string, string>; body?: Buffer } = {}
): Promise<Answer> {
	return new Promise((resolve, reject) => {
		const req = http.request(`${relayUrl}${path}`, {
			method: opts.method ?? 'GET',
			headers: { host: 'smiths.nolune.test', ...opts.headers }
		});
		req.on('error', reject);
		req.on('response', async (res) => {
			const chunks: Buffer[] = [];
			for await (const chunk of res) chunks.push(chunk);
			resolve({ status: res.statusCode!, headers: res.headers, body: Buffer.concat(chunks) });
		});
		req.end(opts.body);
	});
}

describe('a request through the relay', () => {
	it('reaches the web server as it was made, and its answer comes back', async () => {
		await connect(await register('smiths'));
		respond = (_req, res) => {
			res.setHeader('set-cookie', ['a=1; Path=/', 'b=2; Path=/']);
			res.writeHead(201, { 'content-type': 'text/plain', 'x-answer': 'yes' });
			res.end('made');
		};
		const answer = await request('/api/p/family/things?sort=new', {
			method: 'POST',
			headers: {
				cookie: 'session=abc',
				'content-type': 'application/json',
				'x-forwarded-for': '6.6.6.6',
				forwarded: 'for=6.6.6.6'
			},
			body: Buffer.from('{"a":1}')
		});

		expect(answer.status).toBe(201);
		expect(answer.body.toString()).toBe('made');
		expect(answer.headers['x-answer']).toBe('yes');
		expect(answer.headers['set-cookie']).toEqual(['a=1; Path=/', 'b=2; Path=/']);

		const [{ req, body }] = received;
		expect(req.method).toBe('POST');
		expect(req.url).toBe('/api/p/family/things?sort=new');
		expect(req.headers.host).toBe('smiths.nolune.test');
		expect(req.headers.cookie).toBe('session=abc');
		// The relay says who asked; what the browser claimed is dropped.
		expect(req.headers['x-forwarded-for']).toBe('127.0.0.1');
		expect(req.headers['x-forwarded-proto']).toBe('http');
		expect(req.headers.forwarded).toBeUndefined();
		expect(body.toString()).toBe('{"a":1}');
	});

	it('carries large uploads and downloads whole', async () => {
		await connect(await register('smiths'));
		respond = (_req, res, body) => res.end(Buffer.concat([body, body]));
		const upload = Buffer.alloc(6 * 1024 * 1024);
		for (let i = 0; i < upload.length; i++) upload[i] = i % 251;

		const answer = await request('/api/p/family/uploads', { method: 'POST', body: upload });
		expect(answer.status).toBe(200);
		expect(answer.body.equals(Buffer.concat([upload, upload]))).toBe(true);
	});

	it('streams event streams as they go, and ends them when the browser leaves', async () => {
		await connect(await register('smiths'));
		respond = (_req, res) => {
			res.writeHead(200, { 'content-type': 'text/event-stream' });
			res.write('data: first\n\n');
		};
		const req = http.get(`${relayUrl}/api/events`, { headers: { host: 'smiths.nolune.test' } });
		const first = await new Promise<string>((resolve) =>
			req.on('response', (res) => res.once('data', (chunk) => resolve(String(chunk))))
		);
		expect(first).toBe('data: first\n\n');
		expect(received[0].open).toBe(true);

		req.destroy();
		await vi.waitFor(() => expect(received[0].open).toBe(false));
	});

	it('serves many requests at once over the one connection', async () => {
		await connect(await register('smiths'));
		respond = (req, res) => setTimeout(() => res.end(req.url), 20);
		const answers = await Promise.all(Array.from({ length: 40 }, (_, i) => request(`/page/${i}`)));
		expect(answers.map((a) => a.body.toString())).toEqual(
			Array.from({ length: 40 }, (_, i) => `/page/${i}`)
		);
	});

	it("shows the relay's page when the web server doesn't answer", async () => {
		await connect(await register('smiths'));
		web.closeAllConnections();
		await new Promise((resolve) => web.close(resolve));
		const answer = await request('/');
		expect(answer.status).toBe(502);
		expect(answer.body.toString()).toContain("nolune didn't answer");
		web.listen(webPort, '127.0.0.1');
	});
});

describe('the connection', () => {
	it('says where nolune is reachable', async () => {
		const { log } = await connect(await register('smiths'));
		await vi.waitFor(() =>
			expect(log).toEqual(['nolune is reachable at http://smiths.nolune.test'])
		);
	});

	it('holds requests while the gateway comes back, and shows the offline page when it stays away', async () => {
		const registration = await register('smiths');
		const { link } = await connect(registration);
		link.close();
		await vi.waitFor(() => expect(relay.online('smiths')).toBe(false));

		// Restarting: the request waits for the gateway to connect again.
		const waiting = request('/after-restart');
		await new Promise((resolve) => setTimeout(resolve, 100));
		await connect(registration);
		expect((await waiting).body.toString()).toBe('hello');

		// Gone for longer than the grace period: the offline page, and once the period is over,
		// at once.
		links.splice(0).forEach((l) => l.close());
		await vi.waitFor(() => expect(relay.online('smiths')).toBe(false));
		expect((await request('/')).status).toBe(503);
		const started = Date.now();
		expect((await request('/')).status).toBe(503);
		expect(Date.now() - started).toBeLessThan(500);
	});

	it('gives way to another gateway with the same address, and stops', async () => {
		const registration = await register('smiths');
		const first = await connect(registration);
		await connect(registration);
		await vi.waitFor(() => expect(first.log.at(-1)).toMatch(/took it over/));
		expect(relay.online('smiths')).toBe(true);
	});

	it('stops when its address is given back', async () => {
		const registration = await register('smiths');
		const { log } = await connect(registration);
		await fetch(`${relayUrl}/api/gateways/smiths`, {
			method: 'DELETE',
			headers: { authorization: `Bearer ${registration.token}` }
		});
		await vi.waitFor(() => expect(log.at(-1)).toMatch(/given back/));
	});

	it("keeps trying a relay it can't reach, and says so once", async () => {
		const log: string[] = [];
		links.push(
			connectRelay(
				{ server: 'http://127.0.0.1:1', name: 'smiths', url: '', token: 't' },
				{ host: '127.0.0.1', port: webPort },
				(message) => log.push(message)
			)
		);
		await vi.waitFor(
			() => expect(log).toEqual(["can't reach the relay at http://127.0.0.1:1; trying again"]),
			{
				timeout: 5000
			}
		);
	});
});

describe('nolune relay', () => {
	it('gets an address, reports it, and gives it back', async () => {
		const enable = testIo();
		expect(
			await runCli(['relay', 'enable', '--name', 'smiths', '--server', relayUrl], enable.io)
		).toBe(0);
		expect(enable.out()).toContain("nolune's address: http://smiths.nolune.test");
		expect(readConfig().relay).toMatchObject({ server: relayUrl, name: 'smiths' });
		// The address is nolune's origin while the relay is on.
		expect(webhookUrl('abc')).toBe('http://smiths.nolune.test/api/hooks/abc');

		const again = testIo();
		await runCli(['relay', 'enable'], again.io);
		expect(again.out()).toContain('already on: http://smiths.nolune.test');

		const status = testIo();
		await runCli(['relay', 'status'], status.io);
		expect(status.out()).toContain('Address  http://smiths.nolune.test');
		expect(status.out()).toContain('Gateway  not connected');

		const disable = testIo();
		expect(await runCli(['relay', 'disable'], disable.io)).toBe(0);
		expect(disable.out()).toContain('no longer leads here');
		expect(readConfig().relay).toBeUndefined();
		// Given back: the relay has the name free again.
		expect((await register('smiths')).name).toBe('smiths');
	});

	it('gives the old name back when it gets a new one', async () => {
		const { io } = testIo();
		await runCli(['relay', 'enable', '--name', 'smiths', '--server', relayUrl], io);
		await runCli(['relay', 'enable', '--name', 'jones'], io);
		expect(readConfig().relay?.name).toBe('jones');
		expect((await register('smiths')).name).toBe('smiths');
	});

	it('asks again for an address the relay forgot', async () => {
		const { io } = testIo();
		await runCli(['relay', 'enable', '--name', 'smiths', '--server', relayUrl], io);
		const forgotten = readConfig().relay!;
		await fetch(`${relayUrl}/api/gateways/smiths`, {
			method: 'DELETE',
			headers: { authorization: `Bearer ${forgotten.token}` }
		});

		const again = testIo();
		expect(await runCli(['relay', 'enable'], again.io)).toBe(0);
		expect(again.out()).toContain("nolune's address: http://smiths.nolune.test");
		expect(readConfig().relay?.token).not.toBe(forgotten.token);
		await connect(readConfig().relay!);
	});

	it("says what's wrong with a name the relay won't give", async () => {
		await register('smiths');
		const taken = testIo();
		expect(
			await runCli(['relay', 'enable', '--name', 'smiths', '--server', relayUrl], taken.io)
		).toBe(1);
		expect(taken.err()).toContain('smiths is taken');

		const invalid = testIo();
		expect(
			await runCli(['relay', 'enable', '--name', 'a.b', '--server', relayUrl], invalid.io)
		).toBe(1);
		expect(invalid.err()).toContain('a name has 3 to 32');
		expect(readConfig().relay).toBeUndefined();
	});

	it('is part of setup', async () => {
		const { io, out } = testIo({ stdin: '', env: { NOLUNE_RELAY_SERVER: relayUrl } });
		const code = await runCli(
			[
				'setup',
				'--name',
				'Anna',
				'--email',
				'anna@example.com',
				'--password',
				'a-long-and-strong-password-1',
				'--relay',
				'--relay-name',
				'smiths'
			],
			io
		);
		expect(code).toBe(0);
		expect(out()).toContain('Address: http://smiths.nolune.test');
		expect(out()).toContain('open http://smiths.nolune.test');
		expect(readConfig().relay?.name).toBe('smiths');
	});

	it("isn't part of setup without a terminal, unless asked for", async () => {
		const { io, out } = testIo({ stdin: '' });
		await runCli(
			[
				'setup',
				'--name',
				'Anna',
				'--email',
				'anna@example.com',
				'--password',
				'a-long-and-strong-password-1'
			],
			io
		);
		expect(readConfig().relay).toBeUndefined();
		expect(out()).toContain('nolune relay enable');
	});
});
