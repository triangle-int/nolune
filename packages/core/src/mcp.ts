import { execFile } from 'node:child_process';
import { homedir } from 'node:os';
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
import { paths } from './paths.ts';
import { commandEnv, commandShell, resolveCwd } from './run-command.ts';
import { NOLUNE_VERSION } from './updates.ts';

/*
 * MCP servers: the tools of other apps and services (GitHub, Notion, Home Assistant, a browser...)
 * that the family connects to nolune. The agent keeps its one tool: it lists and calls theirs with
 * `nolune mcp` (packages/cli/src/mcp.ts), a command like any other, so every provider and both
 * plans get them, auto mode checks each call, and a chat's tools and prompt cache stay as they
 * are. A built-in skill (`mcp`) says how, and a chat's prompt names the servers its profile has.
 *
 * Admins connect them on the Connected services page or with `nolune mcp add`. config.json keeps them the way
 * MCP clients write them (`mcpServers`: a command this computer runs, or an address), so a
 * server's own instructions can be pasted, plus what nolune adds: a description for the agent and
 * the profiles that have it (all, when it names none).
 *
 * The gateway, which runs the agent's `nolune` commands, keeps each server's connection open
 * between calls (a command's server keeps running), and closes it after IDLE_MS unused or when
 * the server's settings change. A `nolune` that runs a command itself connects for it and closes
 * after it.
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
	use: (client: Client) => Promise<T>
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
		return await use(connection.client);
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
	return withMcpServer(name, options, (client) => describe(client, options.signal));
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
	const { client } = await connect(name, parseMcpServer(server), signal);
	try {
		return await describe(client, signal);
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

/**
 * For a chat's prompt: the servers its profile has, each with what it's for. Empty when there
 * are none.
 */
export function mcpServersSection(profile: string): string {
	const servers = listMcpServers(profile).filter((s) => !s.problem);
	return servers.map((s) => `- ${s.name}${s.description ? `: ${s.description}` : ''}`).join('\n');
}
