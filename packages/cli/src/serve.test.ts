import { execFile } from 'node:child_process';
import { mkdirSync, mkdtempSync, statSync, writeFileSync } from 'node:fs';
import { connect, type Server, type Socket } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createProfile, initConfig, readMemoryNote } from '@nolune/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeUser } from '../../core/src/test/fixtures.ts';
import { runInGateway, type Stdio } from './client.ts';
import type { Io } from './io.ts';
import { PROTOCOL, send } from './protocol.ts';
import { runCli } from './run.ts';
import { serveCommands } from './serve.ts';

/*
 * The gateway running `nolune` commands for the CLI over its socket: what goes through, what stays
 * local, and what happens when either side goes away.
 */

let servers: Server[] = [];

beforeEach(() => {
	initConfig();
});

afterEach(async () => {
	await Promise.all(servers.map((s) => new Promise((resolve) => s.close(resolve))));
	servers = [];
	vi.restoreAllMocks();
});

/** A socket path in a fresh folder, short enough for a Unix socket. */
function socketPath(): string {
	return join(mkdtempSync(join(tmpdir(), 'nolune-sock-')), 'run', 'cli.sock');
}

async function serve(path: string, run: (argv: string[], io: Io) => Promise<number> = runCli) {
	const server = await serveCommands(path, () => run);
	if (server) servers.push(server);
	return server;
}

function stdio(stdin?: string) {
	const out: string[] = [];
	const err: string[] = [];
	const readStdin = vi.fn(async () => stdin ?? '');
	const io: Stdio = { stdout: (t) => out.push(t), stderr: (t) => err.push(t), readStdin };
	return { io, readStdin, out: () => out.join(''), err: () => err.join('') };
}

function ask(path: string, argv: string[], s = stdio(), env: Record<string, string> = {}) {
	return runInGateway(argv, { socketPath: path, cwd: '/', env, stdio: s.io });
}

describe('the command socket', () => {
	it("runs a command with the client's environment and streams its output back", async () => {
		createProfile('Family', makeUser('Anna').id);
		const path = socketPath();
		await serve(path);
		const s = stdio();

		const code = await ask(path, ['memory', 'add', 'family', 'Anna is 7'], s, {
			NOLUNE_PROFILE: 'family'
		});

		expect(code).toBe(0);
		expect(s.out()).toBe('Started family.md.\n');
		expect(readMemoryNote('family', 'family').text).toContain('Anna is 7');
	});

	it('passes on errors and exit codes', async () => {
		const path = socketPath();
		await serve(path);
		const s = stdio();
		expect(await ask(path, ['frobnicate'], s)).toBe(1);
		expect(s.err()).toBe('nolune: unknown command "frobnicate". See `nolune help`.\n');
	});

	it('sends stdin only to a command that reads it', async () => {
		createProfile('Family', makeUser('Anna').id);
		const path = socketPath();
		await serve(path);
		const env = { NOLUNE_PROFILE: 'family' };

		const writing = stdio('- Loves drawing\n');
		expect(await ask(path, ['memory', 'write', 'people/anna'], writing, env)).toBe(0);
		expect(writing.readStdin).toHaveBeenCalledTimes(1);
		expect(readMemoryNote('family', 'people/anna').text).toBe('- Loves drawing\n');

		const showing = stdio();
		expect(await ask(path, ['memory', 'show', 'people/anna'], showing, env)).toBe(0);
		expect(showing.readStdin).not.toHaveBeenCalled();
	});

	it('leaves the command to the CLI when nothing serves the socket', async () => {
		const path = socketPath();
		expect(await ask(path, ['memory', 'list'])).toBeNull();

		// Left by a gateway that was killed: nothing answers on it, and the next one replaces it.
		mkdirSync(dirname(path), { recursive: true });
		writeFileSync(path, '');
		expect(await ask(path, ['memory', 'list'])).toBeNull();
		expect(await serve(path)).not.toBeNull();
		expect(await ask(path, ['help'])).toBe(0);
	});

	it('keeps setup, start and service in their own process', async () => {
		const path = socketPath();
		const run = vi.fn(runCli);
		await serve(path, run);
		for (const argv of [['setup'], ['start'], ['service', 'status'], ['init']]) {
			expect(await ask(path, argv)).toBeNull();
		}

		// Even from a client that asks anyway.
		const declined = await new Promise<string>((resolve) => {
			const socket = connect(path);
			let reply = '';
			socket.on('connect', () =>
				send(socket, { type: 'run', protocol: PROTOCOL, argv: ['start'], cwd: '/', env: {} })
			);
			socket.on('data', (chunk) => (reply += chunk));
			socket.on('close', () => resolve(reply));
		});
		expect(JSON.parse(declined)).toMatchObject({ type: 'decline' });
		expect(run).not.toHaveBeenCalled();
	});

	it('declines another protocol version, so the CLI runs the command itself', async () => {
		const path = socketPath();
		await serve(path);
		const reply = await new Promise<string>((resolve) => {
			const socket = connect(path);
			let data = '';
			socket.on('connect', () =>
				send(socket, { type: 'run', protocol: PROTOCOL + 1, argv: ['help'], cwd: '/', env: {} })
			);
			socket.on('data', (chunk) => (data += chunk));
			socket.on('close', () => resolve(data));
		});
		expect(JSON.parse(reply)).toMatchObject({ type: 'decline' });
	});

	it('drops a connection that sends nonsense, and goes on serving', async () => {
		const path = socketPath();
		await serve(path);
		vi.spyOn(console, 'error').mockImplementation(() => {});
		for (const line of ['null', '123', '"run"', 'not json', '{"type":"run","argv":"help"}']) {
			const closed = await new Promise<boolean>((resolve) => {
				const socket = connect(path);
				socket.on('connect', () => socket.write(`${line}\n`));
				socket.on('error', () => {});
				socket.on('close', () => resolve(true));
			});
			expect(closed).toBe(true);
		}
		expect(await ask(path, ['help'])).toBe(0);
	});

	it('stops the command when the client hangs up', async () => {
		const path = socketPath();
		let aborted!: () => void;
		const stopped = new Promise<void>((resolve) => (aborted = resolve));
		await serve(path, async (_argv, io) => {
			io.signal.addEventListener('abort', () => aborted());
			await new Promise(() => {});
			return 0;
		});

		const socket = connect(path);
		socket.on('connect', () => {
			send(socket, {
				type: 'run',
				protocol: PROTOCOL,
				argv: ['agent', 'watch'],
				cwd: '/',
				env: {}
			});
			setTimeout(() => socket.destroy(), 20);
		});
		await expect(stopped).resolves.toBeUndefined();
	});

	it("reports a gateway that goes away mid-command, and doesn't run it again", async () => {
		const path = socketPath();
		const server = await serve(path, async (_argv, io) => {
			io.stdout('half');
			setTimeout(() => {
				server!.close();
				for (const socket of sockets) socket.destroy();
			}, 20);
			await new Promise(() => {});
			return 0;
		});
		const sockets = new Set<Socket>();
		server!.on('connection', (socket) => sockets.add(socket));
		const s = stdio();

		expect(await ask(path, ['memory', 'add', 'family', 'x'], s)).toBe(1);
		expect(s.out()).toBe('half');
		expect(s.err()).toBe('nolune: the gateway stopped before the command finished.\n');
	});

	it('lets only this user connect, and never takes over from a gateway that serves already', async () => {
		const path = socketPath();
		expect(await serve(path)).not.toBeNull();
		expect(statSync(join(path, '..')).mode & 0o777).toBe(0o700);
		expect(statSync(path).mode & 0o777).toBe(0o600);

		vi.spyOn(console, 'log').mockImplementation(() => {});
		expect(await serveCommands(path, () => runCli)).toBeNull();
		const s = stdio();
		expect(await ask(path, ['help'], s)).toBe(0);
		expect(s.out()).toMatch(/^nolune - a family agent/);
	});

	it("doesn't keep its process running: the gateway exits once its HTTP server closes", async () => {
		const serve = fileURLToPath(new URL('./serve.ts', import.meta.url));
		const script = `import { serveCommands } from ${JSON.stringify(serve)};
			const server = await serveCommands(${JSON.stringify(socketPath())}, () => async () => 0);
			console.log(server ? 'serving' : 'not serving');`;
		const { code, stdout } = await new Promise<{ code: number | null; stdout: string }>(
			(resolve) => {
				const child = execFile(
					process.execPath,
					['--no-warnings', '--input-type=module', '-e', script],
					{
						timeout: 20_000,
						env: { ...process.env, NOLUNE_HOME: mkdtempSync(join(tmpdir(), 'nolune-home-')) }
					},
					(_err, stdout) => resolve({ code: child.exitCode, stdout })
				);
			}
		);
		expect(stdout.trim()).toBe('serving');
		expect(code).toBe(0);
	}, 30_000);
});
