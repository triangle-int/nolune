import { statSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join } from 'node:path';
import type Anthropic from '@anthropic-ai/sdk';
import type {
	AccountInfo,
	Options,
	Query,
	SDKMessage,
	SDKUserMessage
} from '@anthropic-ai/claude-agent-sdk';
import type { ZodType } from 'zod';
import { readConfig } from './config.ts';
import { replyBlocks, toolCalls } from './content-blocks.ts';
import type { Usage } from './conversations.ts';
import type { Effort, ModelReply, StreamEvent, ToolCall } from './models.ts';
import { PlanError, describePlanAccount, type PlanStatus } from './plans.ts';

/*
 * Chats on the Claude plan: the Pro or Max subscription someone signed in to Claude Code with on
 * this computer. btw never sees that login. It runs the Claude Code that's installed here,
 * unmodified, through the Claude Agent SDK, and Claude Code signs in and bills the plan itself.
 *
 * Unlike the API providers, Claude Code runs the agent loop: it keeps the conversation (a session
 * under ~/.claude/projects, resumed on every turn), calls the model and asks btw to run each
 * command through an MCP tool that has the conversation's own `run_command` definition. btw
 * turns what it streams into the same rows and live events as its own loop, so the chat looks the
 * same. Claude Code's built-in tools, settings, CLAUDE.md files, skills and MCP servers are all
 * left out: the model gets btw's system prompt and btw's tool, nothing else.
 */

type Sdk = typeof import('@anthropic-ai/claude-agent-sdk');
type Zod = typeof import('zod');

/** Loaded on first use, like OpenAI's SDK: most `btw` commands never run a chat. */
let loaded: { sdk: Sdk; z: Zod['z'] } | undefined;

async function load(): Promise<{ sdk: Sdk; z: Zod['z'] }> {
	if (!loaded) {
		const [sdk, zod] = await Promise.all([import('@anthropic-ai/claude-agent-sdk'), import('zod')]);
		loaded = { sdk, z: zod.z };
	}
	return loaded;
}

/** The MCP server btw's tools are on, which makes their names `mcp__btw__<name>` in Claude Code. */
const SERVER = 'btw';
const TOOL_PREFIX = `mcp__${SERVER}__`;
/** Commands have their own timeouts (at most 30 minutes); this only stops Claude Code's default. */
const TOOL_TIMEOUT_MS = 2 * 60 * 60 * 1000;
/** After Stop, how long Claude Code gets to end the turn before its process is closed. */
const INTERRUPT_GRACE_MS = 5000;
const STATUS_TIMEOUT_MS = 30_000;

class PlanAbortError extends Error {
	constructor() {
		super('Stopped');
	}
}

export function isPlanAbortError(err: unknown): boolean {
	return err instanceof PlanAbortError;
}

const HOW_TO_SIGN_IN =
	'Run `claude` in a terminal on the computer btw runs on and sign in with your Claude account (/login), or run `btw claude-plan setup` there.';

/** Anthropic's installer for macOS and Linux (code.claude.com/docs/en/setup). */
export const CLAUDE_INSTALL_COMMAND = 'curl -fsSL https://claude.ai/install.sh | bash';

const HOW_TO_INSTALL = `Install it on the computer btw runs on with \`${CLAUDE_INSTALL_COMMAND}\` (or \`brew install --cask claude-code\`) and sign in with your Claude account, or run \`btw claude-plan setup\` there, which does both. If it's installed somewhere btw doesn't look, set its path with \`btw config set claude-path <path>\`.`;

// --- finding and starting Claude Code ---

function isFile(path: string): boolean {
	try {
		return statSync(path).isFile();
	} catch {
		return false;
	}
}

/**
 * The `claude` to run: config's `claudePath`, else the first on the PATH or in the folders its
 * installers use. The gateway may run without the PATH of a login shell (as a LaunchAgent), so
 * those are looked in too. Null when there's none.
 */
export function claudeExecutable(): string | null {
	try {
		const configured = readConfig().claudePath;
		if (configured) return configured;
	} catch {
		// not set up yet
	}
	const home = homedir();
	const candidates = [
		...(process.env.PATH ?? '').split(delimiter).filter(Boolean),
		join(home, '.local', 'bin'),
		join(home, '.claude', 'local'),
		'/opt/homebrew/bin',
		'/usr/local/bin'
	].map((dir) => join(dir, 'claude'));
	return candidates.find(isFile) ?? null;
}

/** The `claude` btw would run, and whether it's there (a configured path may not be). */
export function findClaudeCode(): { path: string | null; installed: boolean } {
	const path = claudeExecutable();
	return { path, installed: !!path && isFile(path) };
}

/** The `claude` to run. Throws, saying how to install it, when there's none. */
function requireClaudeExecutable(): string {
	const path = claudeExecutable();
	if (path && isFile(path)) return path;
	if (path) {
		throw new PlanError(
			`There's no Claude Code at ${path}, where \`btw config set claude-path\` says it is. ${HOW_TO_INSTALL}`
		);
	}
	throw new PlanError(
		`Claude Code isn't installed on this computer, or btw can't find it. Chats on the Claude plan run through it. ${HOW_TO_INSTALL}`
	);
}

/**
 * btw's environment, without the API credentials Claude Code would otherwise use (and bill)
 * instead of the plan.
 */
export function claudeEnv(): Record<string, string | undefined> {
	const env: Record<string, string | undefined> = {
		...process.env,
		CLAUDE_AGENT_SDK_CLIENT_APP: 'btw-agent'
	};
	delete env.ANTHROPIC_API_KEY;
	delete env.ANTHROPIC_AUTH_TOKEN;
	// A gateway started from inside a Claude Code session isn't a nested one.
	delete env.CLAUDECODE;
	return env;
}

/** What every query shares: btw's own prompt and tools only, nothing from Claude Code's settings. */
function baseOptions(cwd: string): Options {
	return {
		cwd,
		env: claudeEnv(),
		// Always the one installed here: the SDK would otherwise try the copy that comes with it,
		// which the published package doesn't carry.
		pathToClaudeCodeExecutable: requireClaudeExecutable(),
		tools: [],
		settingSources: [],
		strictMcpConfig: true,
		permissionMode: 'dontAsk'
	};
}

/** Haiku predates adaptive thinking and effort. Claude Code also takes aliases, like `haiku`. */
function supportsEffort(model: string): boolean {
	return !/haiku/i.test(model);
}

function modelOptions(model: string, effort: Effort): Partial<Options> {
	if (!supportsEffort(model)) return { model };
	// "summarized" also returns the short notes newer models write between tool calls.
	return { model, effort, thinking: { type: 'adaptive', display: 'summarized' } };
}

/** The SDK's errors from starting Claude Code, in words for the people using btw. */
function startError(err: unknown): PlanError {
	if (err instanceof PlanError) return err;
	const message = err instanceof Error ? err.message : String(err);
	if (/not found|failed to launch|ENOENT|EACCES/i.test(message)) {
		return new PlanError(
			`Couldn't start Claude Code (${message.split('\n')[0]}). ${HOW_TO_INSTALL}`
		);
	}
	return new PlanError(`Claude Code stopped: ${message.split('\n')[0]}`);
}

/** Claude Code's own error text, with how to fix the common ones. */
function turnError(text: string, kind: string | null): PlanError {
	if (kind === 'authentication_failed' || /\/login|not logged in|invalid api key/i.test(text)) {
		return new PlanError(
			`Claude Code isn't signed in to a Claude plan (${text}). ${HOW_TO_SIGN_IN}`,
			kind
		);
	}
	return new PlanError(text, kind);
}

// --- a chat's turn ---

export interface PlanTurn {
	/** The chat's Claude Code session, and whether it exists yet (else it's created with this id). */
	sessionId: string;
	resume: boolean;
	/** Claude Code's working folder: the profile's. */
	cwd: string;
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	/** What the model hasn't seen yet, sent as one message. */
	input: Anthropic.ContentBlockParam[];
	signal: AbortSignal;
	/** Claude Code took the input: from here on it's in the session, even if the turn fails. */
	onStarted: () => void;
	onEvent: (event: StreamEvent) => void;
	/** A model call ended: its reply, to save. Its commands wait until this resolves. */
	onReply: (reply: ModelReply) => Promise<void>;
	/** Runs a call of the reply that was saved last. */
	runTool: (call: ToolCall) => Promise<Anthropic.ToolResultBlockParam>;
	/** Every call of the reply that was saved last has its result, in the reply's order. */
	onResults: (results: Anthropic.ToolResultBlockParam[]) => void;
}

interface Deferred {
	promise: Promise<void>;
	resolve: () => void;
	reject: (err: unknown) => void;
}

function deferred(): Deferred {
	let resolve!: () => void;
	let reject!: (err: unknown) => void;
	const promise = new Promise<void>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	// Handlers that never ask for it mustn't turn a rejection into an unhandled one.
	promise.catch(() => {});
	return { promise, resolve, reject };
}

/** A reply as it comes in. Claude Code sends each block as a message of its own. */
interface OpenReply {
	messageId: string;
	/** Seen from its message_start: complete at message_stop. Otherwise it came whole. */
	streamed: boolean;
	blocks: Anthropic.ContentBlock[];
	stopReason: string | null;
	usage: Usage;
	saving?: Promise<void>;
}

/** The reply that was saved last, whose calls are waiting for results. */
interface Answering {
	calls: ToolCall[];
	/** btw's own results: what it ran, and what it said about it. */
	ours: Map<string, Anthropic.ToolResultBlockParam>;
	/** What Claude Code recorded for calls btw never got (invalid input, a stop). */
	theirs: Map<string, Anthropic.ToolResultBlockParam>;
	/** Calls whose command is running now. */
	running: Set<string>;
}

type UsageFields = {
	input_tokens?: number | null;
	output_tokens?: number | null;
	cache_read_input_tokens?: number | null;
	cache_creation_input_tokens?: number | null;
};

function usageOf(u: UsageFields | null | undefined): Usage {
	return {
		input: u?.input_tokens ?? 0,
		cacheRead: u?.cache_read_input_tokens ?? 0,
		cacheWrite: u?.cache_creation_input_tokens ?? 0,
		output: u?.output_tokens ?? 0
	};
}

/** A block as btw stores it: `run_command` rather than Claude Code's name for it. */
function storedBlock(block: Anthropic.ContentBlock): Anthropic.ContentBlock {
	if (block.type === 'tool_use' && block.name.startsWith(TOOL_PREFIX)) {
		return { ...block, name: block.name.slice(TOOL_PREFIX.length) };
	}
	return block;
}

function textOf(content: unknown): string {
	return replyBlocks(content)
		.flatMap((b) => (b.type === 'text' ? [b.text] : []))
		.join('\n')
		.trim();
}

/** A command's result as an MCP tool result, which Claude Code turns back into the same blocks. */
function toCallToolResult(result: Anthropic.ToolResultBlockParam) {
	const blocks = typeof result.content === 'string' ? [result.content] : (result.content ?? []);
	const content = blocks.map((b) => {
		if (typeof b === 'string') return { type: 'text' as const, text: b };
		if (b.type === 'image' && b.source.type === 'base64') {
			return { type: 'image' as const, data: b.source.data, mimeType: b.source.media_type };
		}
		return { type: 'text' as const, text: b.type === 'text' ? b.text : `[${b.type}]` };
	});
	return { content, ...(result.is_error ? { isError: true } : {}) };
}

/** An error Claude Code reported instead of a reply. */
interface Failure {
	text: string;
	/** Its kind (`authentication_failed`, `rate_limit`...), when it said. */
	kind: string | null;
}

/** The tool's input schema as the zod shape the SDK takes. btw's tools use plain fields only. */
function zodShape(z: Zod['z'], schema: Anthropic.Tool['input_schema']): Record<string, ZodType> {
	const required = new Set(schema.required ?? []);
	const shape: Record<string, ZodType> = {};
	const properties = (schema.properties ?? {}) as Record<
		string,
		{ type?: string; description?: string }
	>;
	for (const [key, property] of Object.entries(properties)) {
		let field: ZodType =
			property.type === 'integer'
				? z.number().int()
				: property.type === 'number'
					? z.number()
					: property.type === 'boolean'
						? z.boolean()
						: z.string();
		if (property.description) field = field.describe(property.description);
		shape[key] = required.has(key) ? field : field.optional();
	}
	return shape;
}

async function* oneMessage(content: Anthropic.ContentBlockParam[]): AsyncIterable<SDKUserMessage> {
	yield { type: 'user', message: { role: 'user', content }, parent_tool_use_id: null };
}

/**
 * One turn of a chat: Claude Code answers the new input, running commands through `runTool`,
 * until the model ends its turn. Each model call's reply is saved (`onReply`) before its commands
 * run, and their results (`onResults`) once they have all ended, the order btw's own loop keeps.
 * Throws a PlanError when the turn fails and a PlanAbortError when it was stopped.
 */
export async function runTurn(turn: PlanTurn): Promise<void> {
	const { sdk, z } = await load();
	if (turn.signal.aborted) throw new PlanAbortError();

	let open: OpenReply | null = null;
	let answering: Answering | null = null;
	/** Per call: resolves once the reply that has it is saved, so its command can run. */
	const saved = new Map<string, Deferred>();
	/** Calls Claude Code asked btw to run whose reply isn't saved yet. */
	const waiting = new Set<string>();
	/** Calls run one after another, as btw runs them. */
	let queue: Promise<unknown> = Promise.resolve();
	const inFlight = new Set<Promise<unknown>>();
	// Set by the message handler below, which TypeScript's narrowing doesn't follow.
	let failure = null as Failure | null;

	const savedFor = (id: string): Deferred => {
		let d = saved.get(id);
		if (!d) saved.set(id, (d = deferred()));
		return d;
	};

	/** Hands the latest reply's results over once each call has one and no command still runs. */
	const settle = (force = false) => {
		const a = answering;
		if (!a || (!force && a.running.size)) return;
		if (!force && !a.calls.every((c) => a.ours.has(c.id) || a.theirs.has(c.id))) return;
		answering = null;
		turn.onResults(
			a.calls.map(
				(c) =>
					a.ours.get(c.id) ??
					a.theirs.get(c.id) ?? {
						type: 'tool_result',
						tool_use_id: c.id,
						content: 'No result came back from this command. It may or may not have run.',
						is_error: true
					}
			)
		);
	};

	const save = (reply: OpenReply): Promise<void> => {
		reply.saving ??= (async () => {
			if (open === reply) open = null;
			if (!reply.blocks.length) return;
			// A reply still being answered when the next one comes: Claude Code gave up on its calls.
			settle(true);
			const content = reply.blocks.map(storedBlock);
			const calls = toolCalls(content);
			const stopReason = reply.stopReason ?? (calls.length ? 'tool_use' : 'end_turn');
			const texts = replyBlocks(content).flatMap((b) => (b.type === 'text' ? [b.text] : []));
			if (calls.length) {
				answering = { calls, ours: new Map(), theirs: new Map(), running: new Set() };
			}
			try {
				await turn.onReply({ content, stopReason, usage: reply.usage, calls, texts });
			} catch (err) {
				for (const call of calls) savedFor(call.id).reject(err);
				throw err;
			}
			for (const call of calls) savedFor(call.id).resolve();
		})();
		return reply.saving;
	};

	/** Starts a reply for `messageId`, saving the one before it if it's still open. */
	const openReply = async (messageId: string, streamed: boolean, usage: Usage) => {
		if (open && open.messageId !== messageId) await save(open);
		if (!open) open = { messageId, streamed, blocks: [], stopReason: null, usage };
		return open;
	};

	/**
	 * A reply that came whole (Claude Code's non-streaming fallback) has no message_stop: it's
	 * saved once Claude Code asks to run one of its calls.
	 */
	const saveIfWaitedOn = (reply: OpenReply | null) => {
		if (
			reply &&
			!reply.streamed &&
			reply.blocks.some((b) => b.type === 'tool_use' && waiting.has(b.id))
		) {
			void save(reply).catch(() => {});
		}
	};

	/** Claude Code asks btw to run a call. */
	const handle = async (id: string, name: string, input: unknown) => {
		waiting.add(id);
		saveIfWaitedOn(open);
		try {
			await savedFor(id).promise;
		} finally {
			waiting.delete(id);
		}
		const a = answering;
		a?.running.add(id);
		const run = queue.then(() => turn.runTool({ type: 'tool_call', id, name, input }));
		queue = run.catch(() => {});
		inFlight.add(run);
		try {
			const result = await run;
			a?.ours.set(id, result);
			return result;
		} finally {
			inFlight.delete(run);
			a?.running.delete(id);
			settle();
		}
	};

	const tools = turn.tools.map((t) =>
		sdk.tool(t.name, t.description ?? '', zodShape(z, t.input_schema), async (args, extra) => {
			const meta = (extra as { _meta?: Record<string, unknown> } | undefined)?._meta;
			const id = meta?.['claudecode/toolUseId'];
			if (typeof id !== 'string') {
				return { content: [{ type: 'text', text: 'Not run: the call has no id.' }], isError: true };
			}
			try {
				return toCallToolResult(await handle(id, t.name, args));
			} catch (err) {
				const reason = err instanceof Error ? err.message : String(err);
				return { content: [{ type: 'text', text: `Not run: ${reason}` }], isError: true };
			}
		})
	);

	let q: Query;
	try {
		q = sdk.query({
			prompt: oneMessage(turn.input),
			options: {
				...baseOptions(turn.cwd),
				...modelOptions(turn.model, turn.effort),
				systemPrompt: turn.system,
				mcpServers: {
					[SERVER]: sdk.createSdkMcpServer({ name: SERVER, timeout: TOOL_TIMEOUT_MS, tools })
				},
				allowedTools: turn.tools.map((t) => TOOL_PREFIX + t.name),
				includePartialMessages: true,
				...(turn.resume ? { resume: turn.sessionId } : { sessionId: turn.sessionId })
			}
		});
	} catch (err) {
		throw startError(err);
	}

	// Stop: Claude Code ends the turn itself (btw's commands are stopped by the same signal), and
	// is closed if it doesn't.
	let closeTimer: NodeJS.Timeout | undefined;
	const onAbort = () => {
		q.interrupt().catch(() => {});
		closeTimer = setTimeout(() => q.close(), INTERRUPT_GRACE_MS);
	};
	turn.signal.addEventListener('abort', onAbort, { once: true });

	const onMessage = async (message: SDKMessage) => {
		switch (message.type) {
			case 'system':
				if (message.subtype === 'init') turn.onStarted();
				return;
			case 'stream_event': {
				if (message.parent_tool_use_id) return;
				const event = message.event;
				if (event.type === 'message_start') {
					await openReply(event.message.id, true, usageOf(event.message.usage));
				} else if (event.type === 'content_block_start') {
					const b = event.content_block;
					const block =
						b.type === 'text' || b.type === 'thinking'
							? { type: b.type }
							: b.type === 'tool_use'
								? { type: 'tool' as const, id: b.id }
								: null;
					if (block) turn.onEvent({ type: 'block_start', index: event.index, block });
				} else if (event.type === 'content_block_delta') {
					if (event.delta.type === 'text_delta') {
						turn.onEvent({ type: 'delta', index: event.index, text: event.delta.text });
					} else if (event.delta.type === 'thinking_delta') {
						turn.onEvent({ type: 'delta', index: event.index, text: event.delta.thinking });
					}
				} else if (event.type === 'message_delta' && open) {
					open.stopReason = event.delta.stop_reason ?? open.stopReason;
					const u = event.usage as UsageFields;
					open.usage = {
						input: u.input_tokens ?? open.usage.input,
						cacheRead: u.cache_read_input_tokens ?? open.usage.cacheRead,
						cacheWrite: u.cache_creation_input_tokens ?? open.usage.cacheWrite,
						output: u.output_tokens ?? open.usage.output
					};
				} else if (event.type === 'message_stop' && open) {
					await save(open);
				}
				return;
			}
			case 'assistant': {
				if (message.parent_tool_use_id) return;
				// Errors come as a reply Claude Code wrote itself, not the model.
				if (message.error || message.message.model === '<synthetic>') {
					if (message.error)
						failure ??= { text: textOf(message.message.content), kind: message.error };
					return;
				}
				const reply = await openReply(message.message.id, false, usageOf(message.message.usage));
				reply.blocks.push(...(message.message.content as Anthropic.ContentBlock[]));
				saveIfWaitedOn(reply);
				return;
			}
			case 'user': {
				const content = message.message.content;
				if (message.parent_tool_use_id || typeof content === 'string') return;
				for (const block of content) {
					if (block.type !== 'tool_result' || !answering) continue;
					if (answering.calls.some((c) => c.id === block.tool_use_id)) {
						answering.theirs.set(block.tool_use_id, block);
					}
				}
				settle();
				return;
			}
			case 'result':
				if (message.is_error && !turn.signal.aborted) {
					const text = message.subtype === 'success' ? message.result : message.errors.join('\n');
					failure ??= { text: text || 'Claude Code ended the turn with an error.', kind: null };
				}
				return;
		}
	};

	let thrown: unknown = null;
	try {
		for await (const message of q) await onMessage(message);
	} catch (err) {
		thrown = err;
	} finally {
		turn.signal.removeEventListener('abort', onAbort);
		clearTimeout(closeTimer);
		q.close();
		// A reply cut off mid-stream is dropped, as btw's own loop drops it. Commands still running
		// finish (a stop ends them), and the calls that got nothing are answered.
		open = null;
		for (const d of saved.values()) d.reject(new Error('the turn ended'));
		await Promise.allSettled([...inFlight]);
		settle(true);
	}

	if (turn.signal.aborted) throw new PlanAbortError();
	if (failure) throw turnError(failure.text, failure.kind);
	if (thrown) throw startError(thrown);
}

/**
 * A turn that failed because the chat's session isn't what btw thought: `missing` (Claude Code
 * no longer has it, say its files were deleted) or `taken` (it exists, though btw never saw it
 * start). Null for any other failure.
 */
export function sessionProblem(err: unknown): 'missing' | 'taken' | null {
	if (!(err instanceof PlanError)) return null;
	if (/no conversation found with session id/i.test(err.message)) return 'missing';
	if (/session id .* is already in use/i.test(err.message)) return 'taken';
	return null;
}

// --- short exchanges and the sign-in check ---

/** One short exchange without tools or a session, for chores like naming a chat. */
export async function quickReply(opts: {
	model: string;
	system: string;
	input: string;
	timeoutMs: number;
}): Promise<{ text: string | null; usage: Usage }> {
	const { sdk } = await load();
	const abortController = new AbortController();
	const timer = setTimeout(() => abortController.abort(), opts.timeoutMs);
	let text: string | null = null;
	let usage: Usage = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 };
	// Set inside the loop, which TypeScript's narrowing doesn't follow.
	let failure = null as Failure | null;
	try {
		const q = sdk.query({
			prompt: opts.input,
			options: {
				...baseOptions(homedir()),
				model: opts.model,
				...(supportsEffort(opts.model) ? { effort: 'low' as const } : {}),
				systemPrompt: opts.system,
				persistSession: false,
				maxTurns: 1,
				abortController
			}
		});
		for await (const message of q) {
			if (message.type === 'assistant' && message.error) {
				failure ??= { text: textOf(message.message.content), kind: message.error };
			} else if (message.type === 'result') {
				usage = usageOf(message.usage);
				if (
					!message.is_error &&
					message.subtype === 'success' &&
					message.stop_reason === 'end_turn'
				) {
					text = message.result;
				}
			}
		}
	} catch (err) {
		if (abortController.signal.aborted) throw new PlanError('Claude Code took too long to answer.');
		if (!failure) throw startError(err);
	} finally {
		clearTimeout(timer);
	}
	if (failure) throw turnError(failure.text, failure.kind);
	return { text, usage };
}

export interface ClaudePlanStatus extends PlanStatus {
	/** The Claude Code btw runs, if it found one (or was told where it is). */
	path: string | null;
	/** Whether that Claude Code is there. */
	installed: boolean;
	/** Who Claude Code is signed in as, as it says; null when it couldn't be asked. */
	account: AccountInfo | null;
}

/** How Claude Code is signed in, e.g. "signed in as anna@example.com (Claude Max)". */
export function describeAccount(account: AccountInfo): string {
	if (!account.email && !account.subscriptionType && account.tokenSource) {
		return `signed in with a token (${account.tokenSource})`;
	}
	return describePlanAccount({
		email: account.email ?? null,
		plan: account.subscriptionType ?? null
	});
}

/**
 * What's wrong with how Claude Code is signed in, for chats on the plan. Signed in with a Claude
 * account, it names the plan; with a token from `claude setup-token`, where the token came from.
 * Without either it says "Claude API", which is what it would bill.
 */
function accountProblem(account: AccountInfo): string | null {
	if (account.apiProvider && account.apiProvider !== 'firstParty') {
		return `Claude Code is set up to use ${account.apiProvider}, not a Claude plan.`;
	}
	if (account.apiKeySource && account.apiKeySource !== 'none') {
		return `Claude Code signs in with an API key (${account.apiKeySource}), which bills a Console account rather than a Claude plan. ${HOW_TO_SIGN_IN}`;
	}
	if (/^Claude (Pro|Max|Team|Enterprise)$/.test(account.subscriptionType ?? '')) return null;
	if (account.tokenSource && account.tokenSource !== 'none') return null;
	return `Claude Code isn't signed in to a Claude plan. ${HOW_TO_SIGN_IN}`;
}

/**
 * Starts Claude Code without sending anything and asks who it's signed in as. Nothing is billed.
 */
export async function claudePlanStatus(): Promise<ClaudePlanStatus> {
	const { path, installed } = findClaudeCode();
	const { sdk } = await load();
	let release!: () => void;
	const released = new Promise<void>((resolve) => (release = resolve));
	/** Keeps Claude Code's input open, without a message, until the check is done. */
	async function* nothing(): AsyncIterable<SDKUserMessage> {
		await released;
		yield* [];
	}
	let q: Query | undefined;
	let timer: NodeJS.Timeout | undefined;
	try {
		q = sdk.query({
			prompt: nothing(),
			options: { ...baseOptions(homedir()), persistSession: false }
		});
		const timeout = new Promise<never>((_, reject) => {
			timer = setTimeout(() => reject(new Error('no answer')), STATUS_TIMEOUT_MS);
		});
		const account = await Promise.race([q.accountInfo(), timeout]);
		const problem = accountProblem(account);
		return { path, installed, account, signedIn: describeAccount(account), problem };
	} catch (err) {
		return { path, installed, account: null, signedIn: null, problem: startError(err).message };
	} finally {
		clearTimeout(timer);
		release();
		q?.close();
	}
}

/**
 * How to sign Claude Code in to a Claude plan from a terminal: its own sign-in, which opens
 * Anthropic's page in a browser. btw only starts it; Claude Code keeps what it gets.
 */
export function claudeSignInCommand(path: string): {
	command: string;
	args: string[];
	env: Record<string, string | undefined>;
} {
	return { command: path, args: ['auth', 'login', '--claudeai'], env: claudeEnv() };
}

/** Throws a PlanError unless Claude Code is here and signed in to a plan. */
export async function checkClaudePlan(): Promise<ClaudePlanStatus> {
	const status = await claudePlanStatus();
	if (status.problem) throw new PlanError(status.problem);
	return status;
}
