import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createInterface } from 'node:readline';

/*
 * A connection to `codex app-server`: the JSON-RPC interface OpenAI's Codex serves its own rich
 * clients (its IDE extension) on, one JSON message per line over the process's stdin and stdout
 * (developers.openai.com/codex/app-server). chatgpt-plan.ts says what to ask; this only carries
 * requests, notifications and Codex's own requests to btw.
 */

/** An error Codex answered a request with. */
export class AppServerError extends Error {
	readonly code: number;
	constructor(message: string, code: number) {
		super(message);
		this.code = code;
	}
}

/** The process ended, or couldn't start (`code`: ENOENT, EACCES...). */
export class AppServerExited extends Error {
	readonly code: string | null;
	constructor(message: string, code: string | null = null) {
		super(message);
		this.code = code;
	}
}

export interface AppServerOptions {
	command: string;
	args: string[];
	env: Record<string, string | undefined>;
	/** What the client calls itself in `initialize`: Codex tags its logs and requests with it. */
	clientName: string;
	clientVersion: string;
	onNotification?: (method: string, params: Record<string, unknown>) => void;
	/** Answers a request from Codex; throwing answers it with an error. */
	onRequest?: (method: string, params: Record<string, unknown>) => Promise<unknown>;
}

/** What a client does with what Codex sends on its own. */
export type AppServerHandlers = Pick<AppServerOptions, 'onNotification' | 'onRequest'>;

interface Pending {
	resolve: (result: unknown) => void;
	reject: (err: Error) => void;
}

/** What Codex wrote to stderr last, for when it ends without saying why. */
const STDERR_KEPT = 2000;
/** After close, how long Codex gets to end on its own (saving the thread) before it's stopped. */
const STOP_AFTER_MS = 3000;
/** How long Codex gets to say hello. */
const START_TIMEOUT_MS = 30_000;

export class AppServer {
	private readonly opts: AppServerOptions;
	private readonly proc: ChildProcessWithoutNullStreams;
	private readonly pending = new Map<number, Pending>();
	private nextId = 1;
	private stderr = '';
	private ended: AppServerExited | null = null;
	/** Resolves once the process has ended. */
	readonly exited: Promise<void>;

	private constructor(opts: AppServerOptions) {
		this.opts = opts;
		this.proc = spawn(opts.command, opts.args, { env: opts.env, stdio: ['pipe', 'pipe', 'pipe'] });
		this.exited = new Promise((resolve) => {
			this.proc.once('error', (err: NodeJS.ErrnoException) => {
				this.end(new AppServerExited(err.message, err.code ?? null));
				resolve();
			});
			this.proc.once('close', (code, signal) => {
				const why = this.stderr.trim().split('\n').at(-1);
				this.end(
					new AppServerExited(
						`Codex ended (${signal ?? `exit code ${code}`})${why ? `: ${why}` : ''}`
					)
				);
				resolve();
			});
		});
		this.proc.stderr.setEncoding('utf8');
		this.proc.stderr.on('data', (chunk: string) => {
			this.stderr = (this.stderr + chunk).slice(-STDERR_KEPT);
		});
		// A write after Codex ended fails here; the requests waiting learn it from `close`.
		this.proc.stdin.on('error', () => {});
		createInterface({ input: this.proc.stdout }).on('line', (line) => this.receive(line));
	}

	/** Starts Codex and says hello: nothing else may be sent before. */
	static async start(opts: AppServerOptions): Promise<AppServer> {
		const server = new AppServer(opts);
		let timer: NodeJS.Timeout | undefined;
		try {
			await Promise.race([
				server.request('initialize', {
					clientInfo: { name: opts.clientName, title: 'btw', version: opts.clientVersion },
					// Dynamic tools (btw's own) are still experimental in Codex.
					capabilities: { experimentalApi: true }
				}),
				new Promise<never>((_, reject) => {
					timer = setTimeout(
						() => reject(new Error("Codex didn't start in time.")),
						START_TIMEOUT_MS
					);
				})
			]);
			server.notify('initialized');
		} catch (err) {
			server.close();
			throw err;
		} finally {
			clearTimeout(timer);
		}
		return server;
	}

	request<T = Record<string, unknown>>(method: string, params?: unknown): Promise<T> {
		if (this.ended) return Promise.reject(this.ended);
		const id = this.nextId++;
		return new Promise<T>((resolve, reject) => {
			this.pending.set(id, { resolve: resolve as (result: unknown) => void, reject });
			this.send({ method, id, params });
		});
	}

	notify(method: string, params?: unknown): void {
		this.send({ method, params });
	}

	/**
	 * Ends Codex: its input closes, which ends it. It's stopped if it doesn't, then killed.
	 * Resolves once it has ended.
	 */
	close(): Promise<void> {
		if (this.ended) return this.exited;
		this.proc.stdin.end();
		const stop = setTimeout(() => this.proc.kill('SIGTERM'), STOP_AFTER_MS);
		const kill = setTimeout(() => this.proc.kill('SIGKILL'), 2 * STOP_AFTER_MS);
		stop.unref();
		kill.unref();
		return this.exited.then(() => {
			clearTimeout(stop);
			clearTimeout(kill);
		});
	}

	private send(message: Record<string, unknown>): void {
		if (!this.ended) this.proc.stdin.write(`${JSON.stringify(message)}\n`);
	}

	private end(err: AppServerExited): void {
		this.ended ??= err;
		for (const p of this.pending.values()) p.reject(this.ended);
		this.pending.clear();
	}

	private receive(line: string): void {
		let message: {
			id?: number | string;
			method?: string;
			params?: Record<string, unknown>;
			result?: unknown;
			error?: { code?: number; message?: string };
		};
		try {
			message = JSON.parse(line);
		} catch {
			return; // not a message
		}
		const { id, method } = message;
		if (method === undefined) {
			const p = typeof id === 'number' ? this.pending.get(id) : undefined;
			if (!p) return;
			this.pending.delete(id as number);
			if (message.error) {
				p.reject(
					new AppServerError(message.error.message ?? 'Codex error', message.error.code ?? 0)
				);
			} else {
				p.resolve(message.result ?? {});
			}
			return;
		}
		const params = message.params ?? {};
		if (id === undefined) {
			this.opts.onNotification?.(method, params);
			return;
		}
		const answer = this.opts.onRequest
			? this.opts.onRequest(method, params)
			: Promise.reject(new AppServerError(`btw doesn't answer ${method}`, -32601));
		answer.then(
			(result) => this.send({ id, result }),
			(err: unknown) =>
				this.send({
					id,
					error: {
						code: err instanceof AppServerError ? err.code : -32603,
						message: err instanceof Error ? err.message : String(err)
					}
				})
		);
	}
}
