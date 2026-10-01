import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import type Anthropic from '@anthropic-ai/sdk';
import { UnauthorizedError } from '@modelcontextprotocol/sdk/client/auth.js';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { SSEClientTransport, SseError } from '@modelcontextprotocol/sdk/client/sse.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import {
	StreamableHTTPClientTransport,
	StreamableHTTPError
} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import type { Transport } from '@modelcontextprotocol/sdk/shared/transport.js';
import {
	ErrorCode,
	McpError,
	type CallToolResult,
	type Tool
} from '@modelcontextprotocol/sdk/types.js';
import { readConfig, updateConfig } from './config.ts';
import { viewImage } from './images.ts';
import { paths } from './paths.ts';
import { commandEnv, commandShell, resolveCwd } from './run-command.ts';
import { NOLUNE_VERSION } from './updates.ts';

/*
 * MCP servers: the tools of other apps and services (GitHub, Notion, Home Assistant, a browser...)
 * that the family connects to nolune. A new chat gets its profile's servers' tools next to
 * run_command, as tools of its own (`mcp__<server>__<tool>`, mcpChatTools), saved with it like
 * run_command, and the runner calls them here (callMcpTool). `nolune mcp`
 * (packages/cli/src/mcp.ts) lists and calls them too, for scripts and for servers connected after
 * a chat started.
 *
 * Admins connect them on the Connected services page or with `nolune mcp add`. config.json keeps
 * them the way MCP clients write them (`mcpServers`: a command this computer runs, or an address),
 * so a server's own instructions can be pasted, plus what nolune adds: a description for the
 * agent and the profiles that have it (all, when it names none).
 *
 * The gateway keeps each server's connection open between calls (a command's server keeps
 * running), and closes it after IDLE_MS unused or when the server's settings change. A `nolune`
 * that runs a command itself connects for it and closes after it. What each server last said its
 * tools are is kept in mcp-tools.json, so a chat gets them when it's created without waiting for
 * the servers.
 */

/** How nolune reaches a server: a command it runs, or an address in either of MCP's HTTP ways. */
export type McpTransport = 'stdio' | 'http' | 'sse';
export const MCP_TRANSPORTS: McpTransport[] = ['stdio', 'http', 'sse'];

interface McpServerBase {
	/** What the agent can do with it, in a few words, for the chat's prompt. */
	description?: string;
	/** The slugs of the profiles that have it; all of them when left out. */
	profiles?: string[];
}

/** A server this computer runs, talking over its stdin and stdout. */
export interface McpStdioServer extends McpServerBase {
	type: 'stdio';
	command: string;
	args?: string[];
	/** Its own environment variables, on top of what the agent's commands get. */
	env?: Record<string, string>;
	/** Where it starts: the home folder unless given. */
	cwd?: string;
}

/** A server at an address: Streamable HTTP (`http`) or the older HTTP with SSE (`sse`). */
export interface McpRemoteServer extends McpServerBase {
	type: 'http' | 'sse';
	url: string;
	/** Sent with every request, like `Authorization: Bearer …`. */
	headers?: Record<string, string>;
}

export type McpServerConfig = McpStdioServer | McpRemoteServer;

/** A server's settings, or the user's mistake in them, in words for people and the agent. */
export class McpServerError extends Error {}

const NAME = /^[a-z0-9]+(?:[-_][a-z0-9]+)*$/;
const MAX_NAME = 32;
const MAX_DESCRIPTION = 300;
const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
/** An HTTP header's name (RFC 9110's token). */
const HEADER_NAME = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

/** How long a connection waits for the server to say hello. `npx -y` may download it first. */
const CONNECT_TIMEOUT_MS = 60_000;
/** How long the gateway keeps an unused connection (and a command's server running). */
const IDLE_MS = 10 * 60_000;
/** How much of what a command's server wrote to stderr is kept, to say why it stopped. */
const STDERR_CHARS = 2000;

const SHAPES =
	'{"command": "npx", "args": ["-y", "…"], "env": {"NAME": "value"}} or {"type": "http", "url": "https://…", "headers": {"Authorization": "Bearer …"}}';

function saved(): Record<string, unknown> {
	try {
		return { ...readConfig().mcpServers };
	} catch {
		// not set up yet
		return {};
	}
}

/** Why `name` can't be a server's; null when it can. */
export function mcpServerNameProblem(name: string): string | null {
	return name.length <= MAX_NAME && NAME.test(name)
		? null
		: `A server's name is lowercase letters and digits, with - or _ between words, at most ${MAX_NAME} characters, like github or home-assistant.`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function stringMap(
	value: unknown,
	what: string,
	nameRule: RegExp
): Record<string, string> | undefined {
	if (value === undefined) return undefined;
	if (!isRecord(value)) throw new McpServerError(`${what} is an object of names and values.`);
	const map: Record<string, string> = {};
	for (const [key, item] of Object.entries(value)) {
		if (!nameRule.test(key))
			throw new McpServerError(`${JSON.stringify(key)} can't be ${what}'s name.`);
		if (typeof item !== 'string') throw new McpServerError(`${what} ${key}'s value is text.`);
		map[key] = item;
	}
	return Object.keys(map).length ? map : undefined;
}

export function isMcpAddress(value: string): boolean {
	try {
		const url = new URL(value.trim());
		return url.protocol === 'http:' || url.protocol === 'https:';
	} catch {
		return false;
	}
}

/**
 * A server's settings from JSON as MCP clients write it (Claude's, Cursor's, VS Code's...): a
 * `command` with `args`, `env` and `cwd`, or a `url` (`type` `http`, the default for one, or
 * `sse`) with `headers`. What nolune doesn't use is left out. Throws an McpServerError saying
 * what's wrong.
 */
export function parseMcpServer(value: unknown): McpServerConfig {
	if (!isRecord(value)) throw new McpServerError(`A server is a JSON object, like ${SHAPES}.`);
	const { type, description, profiles } = value;
	const base: McpServerBase = {};
	if (description !== undefined) {
		if (typeof description !== 'string') throw new McpServerError('Its description is text.');
		const tidy = description.trim().replace(/\s+/g, ' ');
		if (tidy.length > MAX_DESCRIPTION) {
			throw new McpServerError(`Its description is at most ${MAX_DESCRIPTION} characters.`);
		}
		if (tidy) base.description = tidy;
	}
	if (profiles !== undefined) {
		if (!Array.isArray(profiles) || !profiles.every((p) => typeof p === 'string' && p)) {
			throw new McpServerError('Its profiles are a list of profile slugs.');
		}
		if (profiles.length) base.profiles = [...new Set(profiles as string[])];
	}
	const remote = type === 'http' || type === 'sse' || type === 'streamable-http';
	if (remote || (type === undefined && value.url !== undefined && value.command === undefined)) {
		const url = value.url;
		if (typeof url !== 'string' || !isMcpAddress(url)) {
			throw new McpServerError('Its url is an address starting with http:// or https://.');
		}
		const headers = stringMap(value.headers, 'A header', HEADER_NAME);
		return {
			type: type === 'sse' ? 'sse' : 'http',
			url: url.trim(),
			...(headers && { headers }),
			...base
		};
	}
	if (type !== undefined && type !== 'stdio') {
		throw new McpServerError(`Its type is stdio, http or sse, not ${JSON.stringify(type)}.`);
	}
	const { command, args, cwd } = value;
	if (typeof command !== 'string' || !command.trim()) {
		throw new McpServerError(`A server needs a command to run or a url, like ${SHAPES}.`);
	}
	if (args !== undefined && (!Array.isArray(args) || !args.every((a) => typeof a === 'string'))) {
		throw new McpServerError('Its args are a list of texts.');
	}
	if (cwd !== undefined && (typeof cwd !== 'string' || !cwd.trim())) {
		throw new McpServerError('Its cwd is a folder.');
	}
	const env = stringMap(value.env, 'An environment variable', ENV_NAME);
	return {
		type: 'stdio',
		command: command.trim(),
		...(args?.length && { args: args as string[] }),
		...(env && { env }),
		...(typeof cwd === 'string' && { cwd: cwd.trim() }),
		...base
	};
}

/** A word as a shell reads it back: quoted when it has to be. */
function shellWord(word: string): string {
	return /^[\w@%+=:,./~-]+$/.test(word) ? word : `'${word.replace(/'/g, `'\\''`)}'`;
}

/** A command and its arguments as one line, quoted the way a shell reads them. */
export function joinCommandLine(words: string[]): string {
	return words.map(shellWord).join(' ');
}

/**
 * The words of a command line, as a shell splits them: spaces between words, '…' as it is, "…"
 * with \" and \\ in it, \ before any character outside quotes. No variables or globs: a server's
 * command runs without a shell.
 */
export function splitCommandLine(line: string): string[] {
	const words: string[] = [];
	let word = '';
	let inWord = false;
	for (let i = 0; i < line.length; i++) {
		const c = line[i];
		if (/\s/.test(c)) {
			if (inWord) words.push(word);
			word = '';
			inWord = false;
			continue;
		}
		inWord = true;
		if (c === "'") {
			const end = line.indexOf("'", i + 1);
			if (end === -1) throw new McpServerError("A ' in the command isn't closed.");
			word += line.slice(i + 1, end);
			i = end;
		} else if (c === '"') {
			let j = i + 1;
			for (; j < line.length && line[j] !== '"'; j++) {
				if (line[j] === '\\' && (line[j + 1] === '"' || line[j + 1] === '\\')) j++;
				word += line[j];
			}
			if (j >= line.length) throw new McpServerError('A " in the command isn\'t closed.');
			i = j;
		} else if (c === '\\' && i + 1 < line.length) {
			word += line[++i];
		} else {
			word += c;
		}
	}
	if (inWord) words.push(word);
	return words;
}

/** What Connected services and `nolune mcp list` show of a server: never its keys and tokens. */
export interface McpServerStatus {
	name: string;
	type: McpTransport;
	/** The command line it runs, or its address. */
	target: string;
	/** Where it starts, for one this computer runs. */
	cwd: string | null;
	/** The names of its environment variables (a command) or headers (an address), not their values. */
	secrets: string[];
	description: string | null;
	/** The profiles that have it, by slug; null for every profile. */
	profiles: string[] | null;
	/** What's wrong with its settings in config.json, when something is. */
	problem: string | null;
}

function status(name: string, raw: unknown): McpServerStatus {
	let server: McpServerConfig;
	try {
		server = parseMcpServer(raw);
	} catch (err) {
		return {
			name,
			type: 'stdio',
			target: '',
			cwd: null,
			secrets: [],
			description: null,
			profiles: null,
			problem: (err as Error).message
		};
	}
	const shared = {
		name,
		type: server.type,
		description: server.description ?? null,
		profiles: server.profiles ?? null,
		problem: null
	};
	return server.type === 'stdio'
		? {
				...shared,
				target: joinCommandLine([server.command, ...(server.args ?? [])]),
				cwd: server.cwd ?? null,
				secrets: Object.keys(server.env ?? {})
			}
		: { ...shared, target: server.url, cwd: null, secrets: Object.keys(server.headers ?? {}) };
}

/** Every server, or those `profile` (a slug) has. */
export function listMcpServers(profile?: string): McpServerStatus[] {
	return Object.entries(saved())
		.map(([name, raw]) => status(name, raw))
		.filter((s) => !profile || !s.profiles || s.profiles.includes(profile));
}

/**
 * The server called `name`, as `profile` (a slug, when the agent asks) can use it. Throws an
 * McpServerError for one that isn't there, isn't the profile's, or whose settings are broken.
 */
export function findMcpServer(name: string, profile?: string): McpServerConfig {
	const all = saved();
	if (!Object.hasOwn(all, name)) {
		const names = Object.keys(all);
		throw new McpServerError(
			`there's no MCP server called ${name}. ${names.length ? `\`nolune mcp list\` shows them.` : 'An admin connects them on the Connected services page, or with `nolune mcp add`.'}`
		);
	}
	let server: McpServerConfig;
	try {
		server = parseMcpServer(all[name]);
	} catch (err) {
		throw new McpServerError(
			`${name}'s settings in config.json are broken: ${(err as Error).message}`
		);
	}
	if (profile && server.profiles && !server.profiles.includes(profile)) {
		throw new McpServerError(
			`${name} isn't connected in this profile. An admin can add it on the Connected services page.`
		);
	}
	return server;
}

/**
 * Adds a server, or replaces the one of that name. `keepSecrets`: a server being changed that
 * has no environment variables or headers in `server` keeps the saved ones, as long as it stays
 * a command or an address (a form never has them). Returns whether it replaced one.
 */
export function saveMcpServer(
	name: string,
	server: McpServerConfig,
	options: { keepSecrets?: boolean } = {}
): { replaced: boolean } {
	const problem = mcpServerNameProblem(name);
	if (problem) throw new McpServerError(problem);
	const next = parseMcpServer(server);
	let replaced = false;
	updateConfig((config) => {
		const all = { ...config.mcpServers };
		replaced = Object.hasOwn(all, name);
		let old: McpServerConfig | null = null;
		try {
			old = replaced ? parseMcpServer(all[name]) : null;
		} catch {
			// broken settings: nothing to keep
		}
		if (options.keepSecrets && old) {
			if (next.type === 'stdio' && old.type === 'stdio') next.env ??= old.env;
			else if (next.type !== 'stdio' && old.type !== 'stdio') next.headers ??= old.headers;
		}
		all[name] = parseMcpServer(next);
		config.mcpServers = all;
	});
	return { replaced };
}

export function removeMcpServer(name: string): void {
	if (!Object.hasOwn(saved(), name))
		throw new McpServerError(`there's no MCP server called ${name}.`);
	updateConfig((config) => {
		const all = { ...config.mcpServers };
		delete all[name];
		if (Object.keys(all).length) config.mcpServers = all;
		else delete config.mcpServers;
	});
	const known = remembered();
	if (Object.hasOwn(known, name)) {
		delete known[name];
		writeRemembered(known);
	}
}

// ---------------------------------------------------------------------------------------------
// Connections

interface Connection {
	/** The settings it was made with: other settings need another connection. */
	key: string;
	client: Client;
	/** A command's server's process, to stop with the gateway. */
	pid: number | null;
	/** Commands using it now. */
	users: number;
	idle: NodeJS.Timeout | null;
	closed: boolean;
	/** Its settings changed while a command used it: it closes once nothing does. */
	stale: boolean;
	/** Its entry in `pool.open`, which a newer connection to the server may have replaced. */
	entry: Promise<Connection>;
}

interface Pool {
	/** Set in the gateway: connections outlive the command that made them. */
	hold: boolean;
	/** Each server's connection, by name, while it's opening and once it's open. */
	open: Map<string, Promise<Connection>>;
	/** The processes of the servers this computer runs, to stop when the gateway exits. */
	pids: Set<number>;
}

// On globalThis, so `pnpm dev` reloading this module keeps the gateway's connections.
const holder = globalThis as unknown as { __noluneMcp?: Pool };
const pool: Pool = (holder.__noluneMcp ??= { hold: false, open: new Map(), pids: new Set() });

/** What a connection depends on: everything but what's only for people and the agent. */
function connectionKey(server: McpServerConfig): string {
	const { description, profiles, ...rest } = server;
	void description;
	void profiles;
	return JSON.stringify(rest);
}

/**
 * The PATH of a login shell, which the gateway (a LaunchAgent, a systemd service) may not have:
 * a server is started with the PATH the agent's commands see, so `npx`, `uvx` and `docker`
 * are found where a terminal finds them.
 */
function loginPath(env: NodeJS.ProcessEnv): Promise<string | null> {
	// After a marker: a profile may print something of its own first.
	const marker = '__NOLUNE_PATH__';
	return new Promise((resolve) => {
		execFile(
			commandShell(),
			['-lc', `printf '%s%s' '${marker}' "$PATH"`],
			{ env, timeout: 10_000, encoding: 'utf8' },
			(err, stdout) => {
				const at = stdout?.lastIndexOf(marker) ?? -1;
				const path = at === -1 ? '' : stdout.slice(at + marker.length).trim();
				resolve(err || !path ? null : path);
			}
		);
	});
}

async function stdioTransport(
	server: McpStdioServer,
	stderr: { text: string }
): Promise<StdioClientTransport> {
	const base = commandEnv(server.env ?? {});
	const path = await loginPath(base);
	const env: Record<string, string> = {};
	for (const [key, value] of Object.entries(base)) if (value !== undefined) env[key] = value;
	if (path) env.PATH = `${paths.bin}:${path}`;
	const transport = new StdioClientTransport({
		command: server.command,
		args: server.args ?? [],
		env,
		cwd: resolveCwd(server.cwd, homedir()),
		stderr: 'pipe'
	});
	transport.stderr?.on('data', (chunk: Buffer) => {
		stderr.text = (stderr.text + chunk.toString('utf8')).slice(-STDERR_CHARS);
	});
	return transport;
}

function remoteTransport(server: McpRemoteServer): Transport {
	const url = new URL(server.url);
	const requestInit: RequestInit = server.headers ? { headers: server.headers } : {};
	return server.type === 'sse'
		? new SSEClientTransport(url, { requestInit })
		: new StreamableHTTPClientTransport(url, { requestInit });
}

/** Why connecting to a server failed, in words that say what to do. */
function connectError(
	name: string,
	server: McpServerConfig,
	err: unknown,
	stderr: string
): McpServerError {
	const said = stderr.trim() ? ` It said: ${stderr.trim().split('\n').slice(-5).join('\n')}` : '';
	if (err instanceof McpError && err.code === ErrorCode.RequestTimeout) {
		return new McpServerError(
			`${name} didn't answer within ${CONNECT_TIMEOUT_MS / 1000} seconds.${said}`
		);
	}
	if (server.type === 'stdio') {
		const message = err instanceof Error ? err.message : String(err);
		if (/ENOENT/.test(message)) {
			return new McpServerError(
				`couldn't start ${name}: there's no ${server.command} on this computer (or on the PATH its commands have). Install it, or give its full path.`
			);
		}
		if (err instanceof McpError && err.code === ErrorCode.ConnectionClosed) {
			return new McpServerError(`${name} stopped before it answered.${said}`);
		}
		return new McpServerError(`couldn't start ${name}: ${message}.${said}`);
	}
	const code = err instanceof StreamableHTTPError || err instanceof SseError ? err.code : undefined;
	if (code === 401 || code === 403 || err instanceof UnauthorizedError) {
		return new McpServerError(
			server.headers
				? `${name} turned nolune away (${code ?? 'unauthorized'}): its key or token wasn't accepted.`
				: `${name} turned nolune away (${code ?? 'unauthorized'}): it wants a key or token, given as a header like "Authorization: Bearer …". Servers that only sign in through a browser (OAuth) can't be connected yet.`
		);
	}
	if (code === 404 || code === 405) {
		return new McpServerError(
			`there's no MCP server at ${server.url} (${code}). Check the address${server.type === 'http' ? '; an older server may need SSE instead' : ''}.`
		);
	}
	const cause = err instanceof Error && err.cause instanceof Error ? err.cause.message : null;
	const message = err instanceof Error ? err.message : String(err);
	return new McpServerError(
		`couldn't reach ${name} at ${server.url}: ${cause ?? message}`.replace(/\.?$/, '.')
	);
}

/** Connects to a server and says hello, or throws an McpServerError saying why it couldn't. */
async function connect(
	name: string,
	server: McpServerConfig,
	signal?: AbortSignal
): Promise<{ client: Client; pid: number | null }> {
	const stderr = { text: '' };
	let transport: Transport | undefined;
	const client = new Client({ name: 'nolune', version: NOLUNE_VERSION }, { capabilities: {} });
	try {
		transport =
			server.type === 'stdio' ? await stdioTransport(server, stderr) : remoteTransport(server);
		await client.connect(transport, { timeout: CONNECT_TIMEOUT_MS, signal });
	} catch (err) {
		await client.close().catch(() => {});
		await transport?.close().catch(() => {});
		if (signal?.aborted) throw signal.reason;
		throw connectError(name, server, err, stderr.text);
	}
	const pid = transport instanceof StdioClientTransport ? transport.pid : null;
	return { client, pid };
}

/** Takes a connection out of the pool, unless a newer one to its server took its place. */
function forget(name: string, connection: Connection): void {
	if (pool.open.get(name) === connection.entry) pool.open.delete(name);
}

async function close(name: string, connection: Connection): Promise<void> {
	forget(name, connection);
	connection.closed = true;
	if (connection.idle) clearTimeout(connection.idle);
	await connection.client.close().catch(() => {});
}

/** The connection to a server, opened if needed; one with other settings is closed first. */
async function acquire(name: string, server: McpServerConfig): Promise<Connection> {
	const key = connectionKey(server);
	const open = pool.open.get(name);
	if (open) {
		const connection = await open.catch(() => null);
		if (connection && !connection.closed && connection.key === key) return connection;
		if (connection && !connection.closed) {
			// Its settings changed: in use, it closes when its commands are done.
			if (connection.users) {
				connection.stale = true;
				forget(name, connection);
			} else await close(name, connection);
		}
		// Another command may have opened the new one meanwhile.
		const newer = pool.open.get(name);
		if (newer && newer !== open) return acquire(name, server);
		if (newer === open) pool.open.delete(name);
	}
	const opening: Promise<Connection> = connect(name, server).then(({ client, pid }) => {
		const connection: Connection = {
			key,
			client,
			pid,
			users: 0,
			idle: null,
			closed: false,
			stale: false,
			entry: opening
		};
		if (pid) pool.pids.add(pid);
		client.onclose = () => {
			connection.closed = true;
			if (pid) pool.pids.delete(pid);
			forget(name, connection);
		};
		// For new chats' tools: what it has now.
		if (pool.hold)
			describe(client).then(
				(found) => remember(name, server, found),
				() => {}
			);
		return connection;
	});
	pool.open.set(name, opening);
	opening.catch(() => {
		if (pool.open.get(name) === opening) pool.open.delete(name);
	});
	return opening;
}

/** A command is done with a connection: the gateway keeps it a while; elsewhere it closes. */
function release(name: string, connection: Connection): void {
	connection.users--;
	if (connection.users > 0 || connection.closed) return;
	if (!pool.hold || connection.stale) {
		void close(name, connection);
		return;
	}
	connection.idle = setTimeout(() => void close(name, connection), IDLE_MS);
	connection.idle.unref();
}

/** Resolves with the promise, or rejects as soon as `signal` aborts. */
function abortable<T>(promise: Promise<T>, signal: AbortSignal | undefined): Promise<T> {
	if (!signal) return promise;
	if (signal.aborted) return Promise.reject(signal.reason);
	return new Promise((resolve, reject) => {
		const onAbort = () => reject(signal.reason);
		signal.addEventListener('abort', onAbort, { once: true });
		promise.then(resolve, reject).finally(() => signal.removeEventListener('abort', onAbort));
	});
}

/**
 * Runs `use` with a connection to the server called `name`, as `profile` (a slug) has it: the
 * gateway's open one, or a new one.
 */
async function withMcpServer<T>(
	name: string,
	options: { profile?: string; signal?: AbortSignal },
	use: (client: Client, server: McpServerConfig) => Promise<T>
): Promise<T> {
	const server = findMcpServer(name, options.profile);
	const acquiring = acquire(name, server);
	let connection: Connection;
	try {
		connection = await abortable(acquiring, options.signal);
	} catch (err) {
		// Stopped while it connected: once it has, it's left as after a command (or closed).
		acquiring.then(
			(c) => {
				c.users++;
				release(name, c);
			},
			() => {}
		);
		throw err;
	}
	connection.users++;
	if (connection.idle) clearTimeout(connection.idle);
	connection.idle = null;
	try {
		return await use(connection.client, server);
	} finally {
		release(name, connection);
	}
}

/**
 * In the gateway: connections stay open between commands, and close when it stops. Called once,
 * where the gateway starts running the agent's commands.
 */
export function holdMcpConnections(): void {
	if (pool.hold) return;
	pool.hold = true;
	// As Node exits, when nothing asynchronous runs any more: a server that doesn't stop when its
	// stdin closes would keep running without its gateway.
	process.once('exit', () => {
		for (const pid of pool.pids) {
			try {
				process.kill(pid, 'SIGTERM');
			} catch {
				// already gone
			}
		}
	});
	process.once('sveltekit:shutdown', () => void closeMcpConnections());
	// Servers whose tools new chats don't know yet, or that changed since: once it has started.
	setTimeout(() => void refreshMcpTools().catch(() => {}), 5000).unref();
}

/** Closes every connection, stopping the servers this computer runs. */
export async function closeMcpConnections(): Promise<void> {
	const open = [...pool.open.entries()];
	pool.open.clear();
	await Promise.all(
		open.map(([name, o]) =>
			o.then(
				(c) => close(name, c),
				() => {}
			)
		)
	);
}

/** What a server says about itself and its tools. */
export interface McpServerTools {
	/** The server's name for itself, like "GitHub MCP Server". */
	title: string | null;
	/** What it tells the model about using it. */
	instructions: string | null;
	tools: Tool[];
}

async function describe(client: Client, signal?: AbortSignal): Promise<McpServerTools> {
	const tools: Tool[] = [];
	let cursor: string | undefined;
	do {
		const page = await client.listTools(cursor ? { cursor } : undefined, { signal });
		tools.push(...page.tools);
		cursor = page.nextCursor;
	} while (cursor);
	const info = client.getServerVersion();
	return {
		title: info?.title ?? info?.name ?? null,
		instructions: client.getInstructions()?.trim() || null,
		tools
	};
}

/** The tools of the server called `name`, with what it says about itself. */
export function listMcpTools(
	name: string,
	options: { profile?: string; signal?: AbortSignal } = {}
): Promise<McpServerTools> {
	return withMcpServer(name, options, async (client, server) => {
		const found = await describe(client, options.signal);
		remember(name, server, found);
		return found;
	});
}

/**
 * Calls a tool. Its result as the server gave it: an error the tool reports is `isError` in it,
 * not thrown. The command running this is what limits how long it takes, so the call itself waits
 * for as long as the server keeps working.
 */
export function callMcpTool(
	name: string,
	tool: string,
	args: Record<string, unknown>,
	options: { profile?: string; signal?: AbortSignal } = {}
): Promise<CallToolResult> {
	return withMcpServer(name, options, async (client) => {
		const result = await client.callTool({ name: tool, arguments: args }, undefined, {
			signal: options.signal,
			timeout: 24 * 60 * 60_000,
			resetTimeoutOnProgress: true
		});
		// Servers on MCP's first version answer with `toolResult` instead.
		if ('toolResult' in result && !('content' in result)) {
			return { content: [{ type: 'text', text: JSON.stringify(result.toolResult, null, 2) }] };
		}
		return result as CallToolResult;
	});
}

/**
 * Connects to a server's settings before they're saved, to check them: what it says about itself
 * and its tools, on a connection of its own that's closed after. Throws an McpServerError.
 */
export async function checkMcpServer(
	name: string,
	server: McpServerConfig,
	signal?: AbortSignal
): Promise<McpServerTools> {
	const parsed = parseMcpServer(server);
	const { client } = await connect(name, parsed, signal);
	try {
		const found = await describe(client, signal);
		// Most likely saved next, and its tools go to new chats.
		remember(name, parsed, found);
		return found;
	} catch (err) {
		throw new McpServerError(
			`${name} connected, but couldn't list its tools: ${(err as Error).message}`
		);
	} finally {
		await client.close().catch(() => {});
	}
}

/** A description from what a server says about itself: the first sentence of its instructions. */
export function describeFromServer(found: McpServerTools): string | undefined {
	const first = found.instructions?.split(/(?<=[.!?])\s|\n/)[0]?.trim();
	return first && first.length <= MAX_DESCRIPTION ? first : undefined;
}

// ---------------------------------------------------------------------------------------------
// Tools for chats

/** What a server said its tools are when nolune last asked (mcp-tools.json), by server name. */
interface Remembered {
	/** The settings it was asked with (connectionKey). */
	key: string;
	instructions: string | null;
	tools: Pick<Tool, 'name' | 'title' | 'description' | 'inputSchema' | 'annotations'>[];
}

function remembered(): Record<string, Remembered> {
	try {
		return JSON.parse(readFileSync(paths.mcpTools, 'utf8')) as Record<string, Remembered>;
	} catch {
		return {};
	}
}

function writeRemembered(all: Record<string, Remembered>): void {
	// Whole, then renamed into place: the gateway and a `nolune` may both write it.
	const temporary = `${paths.mcpTools}.${process.pid}.tmp`;
	writeFileSync(temporary, `${JSON.stringify(all)}\n`);
	renameSync(temporary, paths.mcpTools);
}

function remember(name: string, server: McpServerConfig, found: McpServerTools): void {
	const all = remembered();
	all[name] = {
		key: connectionKey(server),
		instructions: found.instructions,
		tools: found.tools.map(({ name, title, description, inputSchema, annotations }) => ({
			name,
			title,
			description,
			inputSchema,
			annotations
		}))
	};
	writeRemembered(all);
}

/**
 * Asks servers for their tools again: those named, or every server whose tools aren't known yet
 * or were listed with other settings. One that can't be reached is left as it was.
 */
export async function refreshMcpTools(names?: string[]): Promise<void> {
	const known = remembered();
	const stale = listMcpServers().filter((s) => {
		if (s.problem || (names && !names.includes(s.name))) return false;
		return names || known[s.name]?.key !== connectionKey(findMcpServer(s.name));
	});
	await Promise.allSettled(stale.map((s) => listMcpTools(s.name)));
}

const TOOL_PREFIX = 'mcp__';
/**
 * The longest name a chat's tool gets: providers take 64 characters, and on the Claude plan
 * Claude Code puts nolune's tools under `mcp__nolune__`.
 */
const MAX_TOOL_NAME = 51;
const MAX_TOOL_DESCRIPTION = 2048;
/** How many tools of servers a chat gets at most: OpenAI takes 128 in a request. */
export const MAX_CHAT_MCP_TOOLS = 120;
/** How much of what a server says about using it goes in a chat's prompt. */
const MAX_PROMPT_INSTRUCTIONS = 1500;

/**
 * A server's tool as a chat's tool: `mcp__<server>__<tool>`. A tool whose name has characters
 * providers don't take, or that would be too long, ends in a short hash of its own name instead,
 * so no two of a server's tools share one.
 */
export function mcpToolName(server: string, tool: string): string {
	const plain = `${TOOL_PREFIX}${server}__${tool}`;
	if (/^[A-Za-z0-9_-]+$/.test(tool) && plain.length <= MAX_TOOL_NAME) return plain;
	const hash = createHash('sha256').update(tool).digest('hex').slice(0, 6);
	const room = Math.max(1, MAX_TOOL_NAME - TOOL_PREFIX.length - server.length - 2 - 7);
	const cut = tool
		.replace(/[^A-Za-z0-9_-]/g, '_')
		.slice(0, room)
		.replace(/[_-]+$/, '');
	return `${TOOL_PREFIX}${server}__${cut}_${hash}`;
}

/** The server a chat's tool belongs to, or null for nolune's own (run_command). */
export function mcpToolServer(name: string): string | null {
	if (!name.startsWith(TOOL_PREFIX)) return null;
	const rest = name.slice(TOOL_PREFIX.length);
	// Servers' names never have `__` in them.
	const end = rest.indexOf('__');
	return end > 0 ? rest.slice(0, end) : null;
}

function clip(text: string, max: number): string {
	return text.length > max ? `${text.slice(0, max).trimEnd()}…` : text;
}

function toolDefinition(server: string, tool: Remembered['tools'][number]): Anthropic.Tool {
	const { $schema, ...schema } = (tool.inputSchema ?? {}) as Record<string, unknown>;
	void $schema;
	const description =
		tool.description?.trim() || tool.title || tool.annotations?.title || tool.name;
	return {
		name: mcpToolName(server, tool.name),
		description: clip(description, MAX_TOOL_DESCRIPTION),
		input_schema: { ...schema, type: 'object' } as Anthropic.Tool['input_schema']
	};
}

/**
 * The tools a new chat in `profile` (a slug) gets from its servers, as nolune saves tools: each
 * server's whole, as it last said they are, while they fit in MAX_CHAT_MCP_TOOLS. In the gateway,
 * servers whose tools aren't known yet are asked, for the chats after.
 */
export function mcpChatTools(profile: string): Anthropic.Tool[] {
	const known = remembered();
	const tools: Anthropic.Tool[] = [];
	const unknown: string[] = [];
	for (const server of listMcpServers(profile)) {
		if (server.problem) continue;
		const found = known[server.name];
		if (!found) unknown.push(server.name);
		else if (tools.length + found.tools.length <= MAX_CHAT_MCP_TOOLS) {
			tools.push(...found.tools.map((tool) => toolDefinition(server.name, tool)));
		}
	}
	if (unknown.length && pool.hold) void refreshMcpTools(unknown).catch(() => {});
	return tools;
}

/**
 * For a chat's prompt: the servers whose tools are among the chat's (`tools`, as it saved them),
 * with what each is for and says about using its tools, and the profile's others, which it reaches
 * with `nolune mcp`. Empty when the profile has none and the chat has no such tools.
 */
export function mcpToolsSection(tools: readonly Anthropic.Tool[], profile: string): string {
	const inChat = [...new Set(tools.flatMap((t) => mcpToolServer(t.name) ?? []))];
	const servers = listMcpServers(profile).filter((s) => !s.problem);
	const others = servers.filter((s) => !inChat.includes(s.name)).map((s) => s.name);
	if (!inChat.length && !others.length) return '';
	const known = remembered();
	const listed = inChat.map((name) => {
		const description = servers.find((s) => s.name === name)?.description;
		const instructions = known[name]?.instructions;
		return `- ${name}${description ? `: ${description}` : ''}${instructions ? `\n  <instructions>\n${clip(instructions, MAX_PROMPT_INSTRUCTIONS)}\n  </instructions>` : ''}`;
	});
	const elsewhere = others.length
		? `\n\nThe tools of ${others.join(', ')} aren't among yours in this conversation (it was connected later, or there are too many): \`nolune mcp tools <server>\` lists them, \`nolune mcp tools <server> <tool>\` shows what one takes, and \`nolune mcp call <server> <tool> '<json>'\` calls it.`
		: '';
	return `${
		inChat.length
			? `Some of your tools belong to apps and services the family connected to nolune as MCP servers: their names start with mcp__<server>__. They act outside this computer, often in someone's account, so before one sends, posts, books, buys, deletes or changes something, be sure that's what was asked, as you would doing it by hand. Pictures they return are shown to you. The servers, with what each is for and says about using its tools:\n${listed.join('\n')}`
			: 'The family connected apps and services to nolune as MCP servers.'
	}${elsewhere}`;
}

/**
 * A chat's tool call to a server (`mcp__<server>__<tool>`): the server's name and the tool, as it
 * last said it is, or as it says now when that's changed. Throws an McpServerError for a server
 * that's gone or isn't the profile's (a slug), or a tool it no longer has.
 */
export async function findMcpTool(
	name: string,
	options: { profile?: string; signal?: AbortSignal } = {}
): Promise<{ server: string; tool: Remembered['tools'][number] }> {
	const server = mcpToolServer(name);
	if (!server) throw new McpServerError(`${name} isn't a tool of an MCP server.`);
	findMcpServer(server, options.profile);
	const matches = (tool: { name: string }) => mcpToolName(server, tool.name) === name;
	const tool =
		remembered()[server]?.tools.find(matches) ??
		(await listMcpTools(server, options)).tools.find(matches);
	if (!tool) {
		throw new McpServerError(
			`${server} has no such tool any more. \`nolune mcp tools ${server}\` lists the tools it has now.`
		);
	}
	return { server, tool };
}

const EXTENSIONS: Record<string, string> = {
	'image/jpeg': 'jpg',
	'image/svg+xml': 'svg',
	'audio/mpeg': 'mp3',
	'audio/x-wav': 'wav',
	'text/plain': 'txt'
};

function extension(type: string | undefined): string {
	if (!type) return 'bin';
	const known = EXTENSIONS[type];
	if (known) return known;
	const sub = type.split('/')[1]?.split(/[;+]/)[0] ?? '';
	return /^[a-z0-9]{1,8}$/.test(sub) ? sub : 'bin';
}

/**
 * What a tool returned, as text: its text as it is, the rest saved as files in a folder of their
 * own, a line naming each. Pictures are also shown to the agent through `viewDir`, as
 * `nolune view` does, when it's given (a chat's call, or a command of the agent's).
 */
export async function mcpResultText(result: CallToolResult, viewDir?: string): Promise<string> {
	const lines: string[] = [];
	let folder: string | null = null;
	let count = 0;
	const save = (base64: string, type: string | undefined, kind: string) => {
		folder ??= mkdtempSync(join(tmpdir(), 'nolune-mcp-'));
		const file = join(folder, `${kind}-${++count}.${extension(type)}`);
		writeFileSync(file, Buffer.from(base64, 'base64'));
		return file;
	};
	for (const block of result.content) {
		if (block.type === 'text') {
			lines.push(block.text.replace(/\n$/, ''));
		} else if (block.type === 'image') {
			const file = save(block.data, block.mimeType, 'image');
			if (!viewDir) {
				lines.push(`Image: ${file}`);
				continue;
			}
			try {
				lines.push(await viewImage(file, viewDir));
			} catch (err) {
				lines.push(`Image: ${file} (not shown: ${(err as Error).message.replace(/\.+$/, '')})`);
			}
		} else if (block.type === 'audio') {
			lines.push(`Audio: ${save(block.data, block.mimeType, 'audio')}`);
		} else if (block.type === 'resource') {
			const resource = block.resource;
			if ('text' in resource)
				lines.push(`Resource ${resource.uri}:`, resource.text.replace(/\n$/, ''));
			else
				lines.push(`Resource ${resource.uri}: ${save(resource.blob, resource.mimeType, 'file')}`);
		} else if (block.type === 'resource_link') {
			lines.push(
				`Link: ${block.name} ${block.uri}${block.description ? ` (${block.description})` : ''}`
			);
		}
	}
	// A tool with only structured output; one with text too says the same in it.
	if (result.structuredContent && !result.content.some((b) => b.type === 'text')) {
		lines.push(JSON.stringify(result.structuredContent, null, 2));
	}
	return lines.join('\n');
}
