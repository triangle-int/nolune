import { mkdirSync, readFileSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, dirname, join } from 'node:path';
import type Anthropic from '@anthropic-ai/sdk';
import {
	AppServer,
	AppServerError,
	AppServerExited,
	type AppServerHandlers
} from './codex-app-server.ts';
import { readConfig } from './config.ts';
import type { Usage } from './conversations.ts';
import {
	heldElsewhereNote,
	placeholder,
	unresolved,
	type Block,
	type ImageBlock,
	type PdfBlock,
	type TextBlock,
	type ToolResultBlock
} from './format.ts';
import {
	EFFORTS,
	type Effort,
	type ModelChoice,
	type ModelReply,
	type ToolCall
} from './models.ts';
import { packageRoot, paths } from './paths.ts';
import {
	PlanError,
	PlanStopped,
	describePlanAccount,
	type PlanAccount,
	type PlanStatus,
	type PlanTurn,
	type SessionProblem
} from './plans.ts';

/*
 * Chats on the ChatGPT plan: the Plus, Pro, Business... plan someone signed in to Codex with.
 * Like the Claude plan (claude-plan.ts), btw never holds that sign-in. It runs OpenAI's Codex
 * that's installed here, unmodified, through the app server Codex serves its own IDE extension on
 * (codex-app-server.ts), and Codex signs in with ChatGPT, keeps the sign-in fresh and bills the
 * plan itself. Codex keeps its home apart from ~/.codex, in btw's (paths.codexHome), so its
 * settings, sign-in and history are btw's alone.
 *
 * Codex runs the agent loop: it keeps the conversation (a thread in that home, resumed on every
 * turn), calls the model and asks btw to run each command through a dynamic tool with the
 * conversation's own `run_command` definition. btw turns what it streams into the same rows and
 * live events as its own loop, so the chat looks the same. Codex's own tools that act on the
 * computer or the web (shell, patches, pictures, search, apps, plugins, skills, sub-agents) are
 * turned off and AGENTS.md files aren't read: the model gets btw's system prompt and btw's tools,
 * inside Codex's harness, which calls them from short scripts (`exec`).
 */

/** OpenAI's installer for Codex (developers.openai.com/codex/cli). */
export const CODEX_INSTALL_COMMAND = 'npm install -g @openai/codex';

const HOW_TO_INSTALL = `Install it on the computer btw runs on with \`${CODEX_INSTALL_COMMAND}\` (or \`brew install --cask codex\`), then sign in with ChatGPT under Models & keys on the admin page, or run \`btw chatgpt-plan setup\` there, which does both. If it's installed somewhere btw doesn't look, set its path with \`btw config set codex-path <path>\`.`;

export const CHATGPT_SIGN_IN_HELP =
	'Sign in with ChatGPT under Models & keys on the admin page, or run `btw chatgpt-plan setup` on the computer btw runs on.';

/** The namespace btw's tools are in, which makes them `btw__run_command` in Codex's scripts. */
const NAMESPACE = 'btw';
/** After Stop, how long Codex gets to end the turn before it's closed. */
const INTERRUPT_GRACE_MS = 5000;
const STATUS_TIMEOUT_MS = 30_000;
/** How long a device sign-in waits for its code to be entered, as long as OpenAI's code lasts. */
const SIGN_IN_TIMEOUT_MS = 15 * 60 * 1000;

/**
 * Codex's settings for btw's chats, given on its command line so they win over any config.toml:
 * its own tools off, and nothing read from the profile's folder.
 */
const SETTINGS = [
	'features.shell_tool=false',
	'features.unified_exec=false',
	'features.view_image=false',
	'features.image_generation=false',
	'features.browser_use=false',
	'features.computer_use=false',
	'web_search="disabled"',
	'features.apps=false',
	'features.plugins=false',
	'features.tool_suggest=false',
	'features.skill_search=false',
	'skills.bundled.enabled=false',
	'features.goals=false',
	'features.sleep_tool=false',
	'agents.enabled=false',
	'tools.experimental_request_user_input.enabled=false',
	'project_doc_max_bytes=0'
];

// --- finding and starting Codex ---

function isFile(path: string): boolean {
	try {
		return statSync(path).isFile();
	} catch {
		return false;
	}
}

/**
 * The `codex` to run: config's `codexPath`, else the first on the PATH or in the folders its
 * installers use. The gateway may run without the PATH of a login shell (as a LaunchAgent), so
 * those are looked in too, and so is the folder of the Node that runs btw, where npm puts
 * global commands. Null when there's none.
 */
export function codexExecutable(): string | null {
	try {
		const configured = readConfig().codexPath;
		if (configured) return configured;
	} catch {
		// not set up yet
	}
	const home = homedir();
	const candidates = [
		...(process.env.PATH ?? '').split(delimiter).filter(Boolean),
		dirname(process.execPath),
		join(home, '.local', 'bin'),
		join(home, '.npm-global', 'bin'),
		'/opt/homebrew/bin',
		'/usr/local/bin'
	].map((dir) => join(dir, 'codex'));
	return candidates.find(isFile) ?? null;
}

/** The `codex` btw would run, and whether it's there (a configured path may not be). */
export function findCodex(): { path: string | null; installed: boolean } {
	const path = codexExecutable();
	return { path, installed: !!path && isFile(path) };
}

/** The `codex` to run. Throws, saying how to install it, when there's none. */
function requireCodex(): string {
	const path = codexExecutable();
	if (path && isFile(path)) return path;
	if (path) {
		throw new PlanError(
			`There's no Codex at ${path}, where \`btw config set codex-path\` says it is. ${HOW_TO_INSTALL}`
		);
	}
	throw new PlanError(
		`Codex isn't installed on this computer, or btw can't find it. Chats on the ChatGPT plan run through it. ${HOW_TO_INSTALL}`
	);
}

/**
 * btw's environment with Codex's home, without the API keys Codex would otherwise use (and
 * bill) instead of the plan. npm's `codex` is a Node script: the Node that runs btw runs it when
 * the PATH has none.
 */
function codexEnv(): Record<string, string | undefined> {
	const env: Record<string, string | undefined> = { ...process.env, CODEX_HOME: paths.codexHome };
	delete env.OPENAI_API_KEY;
	delete env.CODEX_API_KEY;
	env.PATH = [env.PATH, dirname(process.execPath)].filter(Boolean).join(delimiter);
	return env;
}

let version: string | undefined;

/** btw's version, which Codex adds to what it tells OpenAI about its client. */
function btwVersion(): string {
	try {
		version ??= String(JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8')).version);
	} catch {
		version = '0.0.0';
	}
	return version;
}

/**
 * Codex sets up its home as it starts (its state database), which two starting at once can trip
 * over: btw starts them one at a time, until each has said hello.
 */
let starting: Promise<unknown> = Promise.resolve();
/** Before starting Codex again when it ended while starting. */
const RESTART_AFTER_MS = 500;

/** Starts Codex's app server with btw's settings. */
async function openCodex(handlers: AppServerHandlers = {}): Promise<AppServer> {
	const command = requireCodex();
	mkdirSync(paths.codexHome, { recursive: true });
	const start = () =>
		AppServer.start({
			command,
			args: ['app-server', ...SETTINGS.flatMap((setting) => ['-c', setting])],
			env: codexEnv(),
			clientName: 'btw',
			clientVersion: btwVersion(),
			...handlers
		});
	const opened = starting.then(start).catch(async (err: unknown) => {
		// It ended without saying hello: another Codex (`btw` in a terminal, say) may have been
		// setting up the same home. Once more.
		if (!(err instanceof AppServerExited) || err.code) throw err;
		await new Promise((resolve) => setTimeout(resolve, RESTART_AFTER_MS));
		return start();
	});
	starting = opened.catch(() => {});
	try {
		return await opened;
	} catch (err) {
		throw startError(err);
	}
}

/** What went wrong starting or talking to Codex, in words for the people using btw. */
function startError(err: unknown): PlanError {
	if (err instanceof PlanError) return err;
	if (err instanceof AppServerExited && err.code) {
		return new PlanError(`Couldn't start Codex (${err.message}). ${HOW_TO_INSTALL}`, null, {
			cause: err
		});
	}
	const message = err instanceof Error ? err.message : String(err);
	return new PlanError(`Codex: ${message.split('\n')[0]}`, null, { cause: err });
}

/** Runs `work` with Codex, then closes it. */
async function withCodex<T>(
	work: (codex: AppServer) => Promise<T>,
	handlers: AppServerHandlers = {}
): Promise<T> {
	const codex = await openCodex(handlers);
	try {
		return await work(codex);
	} finally {
		codex.close();
	}
}

// --- what Codex sends and takes ---

interface TokenUsage {
	inputTokens: number;
	cachedInputTokens: number;
	cacheWriteInputTokens?: number;
	outputTokens: number;
}

/** One model call's use, in btw's terms: Codex counts cached input in its input, like OpenAI. */
function usageOf(u: TokenUsage): Usage {
	const cacheRead = u.cachedInputTokens ?? 0;
	const cacheWrite = u.cacheWriteInputTokens ?? 0;
	return {
		input: Math.max(0, (u.inputTokens ?? 0) - cacheRead - cacheWrite),
		cacheRead,
		cacheWrite,
		output: u.outputTokens ?? 0
	};
}

function addUsage(a: Usage | null, b: Usage): Usage {
	if (!a) return b;
	return {
		input: a.input + b.input,
		cacheRead: a.cacheRead + b.cacheRead,
		cacheWrite: a.cacheWrite + b.cacheWrite,
		output: a.output + b.output
	};
}

/** A picture as Codex takes it: a data URL. PDFs and other providers' copies can't go. */
function codexImage(block: ImageBlock | PdfBlock): { url: string } | TextBlock {
	if (block.type === 'pdf') {
		return {
			type: 'text',
			text: `[PDF not shown: models on the ChatGPT plan don't take PDFs. The line before this says where its file is.]`
		};
	}
	// Another provider's Files API holds it, which Codex can't open.
	if (block.source.type === 'uploaded') return heldElsewhereNote(block);
	if (block.source.type === 'media') unresolved(block);
	return { url: `data:${block.source.mime};base64,${block.source.data}` };
}

/**
 * Codex's input items for btw's blocks, with pictures and PDFs resolved (`turn.resolve`). It
 * takes text and pictures.
 */
function toCodexInput(blocks: Block[]): Record<string, unknown>[] {
	return blocks.flatMap((block): Record<string, unknown>[] => {
		const text = (text: string) => [{ type: 'text', text, text_elements: [] }];
		switch (block.type) {
			case 'text':
				return block.text ? text(block.text) : [];
			case 'image':
			case 'pdf': {
				const image = codexImage(block);
				return 'url' in image ? [{ type: 'image', url: image.url }] : text(image.text);
			}
			default:
				// Replies and command results never go as input, and `other` blocks are Claude's.
				return [];
		}
	});
}

/** A command's result, resolved, as Codex takes a dynamic tool's. */
function toToolResponse(result: ToolResultBlock) {
	const blocks = typeof result.content === 'string' ? [result.content] : result.content;
	const contentItems = blocks.map((b) => {
		if (typeof b === 'string') return { type: 'inputText', text: b };
		if (b.type === 'text') return { type: 'inputText', text: b.text };
		if (b.type === 'image' || b.type === 'pdf') {
			const image = codexImage(b);
			return 'url' in image
				? { type: 'inputImage', imageUrl: image.url }
				: { type: 'inputText', text: image.text };
		}
		return { type: 'inputText', text: `[${placeholder(b)}]` };
	});
	return { contentItems, success: !result.isError };
}

/** How Codex's tools see btw's: in btw's namespace, with the same schemas. */
function dynamicTools(tools: Anthropic.Tool[]) {
	if (!tools.length) return [];
	return [
		{
			type: 'namespace',
			name: NAMESPACE,
			description: "btw's tools.",
			tools: tools.map((t) => ({
				type: 'function',
				name: t.name,
				description: t.description ?? '',
				inputSchema: t.input_schema
			}))
		}
	];
}

interface TurnError {
	message?: string;
	codexErrorInfo?: string | Record<string, unknown> | null;
	additionalDetails?: string | null;
}

/** Codex's error for a turn, with how to fix the common ones. */
function turnError(error: TurnError | null): PlanError {
	const info = error?.codexErrorInfo;
	const kind = typeof info === 'string' ? info : info ? (Object.keys(info)[0] ?? null) : null;
	const text = error?.message || 'Codex ended the turn with an error.';
	if (kind === 'unauthorized' || /\b401\b|unauthori[sz]ed|not (logged|signed) in/i.test(text)) {
		return new PlanError(
			`Codex isn't signed in with ChatGPT, or ChatGPT turned its sign-in down (${text}). ${CHATGPT_SIGN_IN_HELP}`,
			kind
		);
	}
	return new PlanError(text, kind);
}

/** An item Codex saves in its thread, as OpenAI's Responses API would have returned it. */
type OutputItem = Record<string, unknown>;

function textsOf(items: OutputItem[]): string[] {
	return items.flatMap((item) =>
		item.type === 'message'
			? ((item.content as { text: string }[] | undefined) ?? []).map((part) => part.text)
			: []
	);
}

// --- a chat's turn ---

/** A turn Codex runs: `onNotification` takes its events, `ended` resolves once Codex ends it. */
interface Following {
	onNotification: (method: string, params: Record<string, unknown>) => void;
	ended: Promise<{ status: string; error: TurnError | null }>;
}

/** Follows the turn Codex runs: its end, and errors it won't retry. Other events go to `on`. */
function follow(on: (method: string, params: Record<string, unknown>) => void): Following {
	let end!: (turn: { status: string; error: TurnError | null }) => void;
	const ended = new Promise<{ status: string; error: TurnError | null }>((resolve) => {
		end = resolve;
	});
	let failure: TurnError | null = null;
	return {
		ended,
		onNotification: (method, params) => {
			if (method === 'error' && !params.willRetry) {
				failure ??= params.error as TurnError;
			} else if (method === 'turn/completed') {
				const turn = params.turn as { status: string; error: TurnError | null };
				end({ status: turn.status, error: turn.error ?? failure });
				return;
			}
			on(method, params);
		}
	};
}

/**
 * One turn of a chat: Codex answers the new input, running commands through `runTool`, until
 * the model ends its turn. Each reply is saved (`onReply`) before its command runs, and its result
 * (`onResults`) once it has ended, the order btw's own loop keeps. Codex asks for one command at
 * a time here, even when a script of its asks for several at once: each gets a reply of its own.
 * Throws a PlanError when the turn fails and a PlanStopped when it was stopped.
 */
export async function runTurn(turn: PlanTurn): Promise<void> {
	if (turn.signal.aborted) throw new PlanStopped();

	/** What the model said since the last reply was saved. */
	let open: OutputItem[] = [];
	/**
	 * What the last model call used. Codex says so once the commands the call asked for have
	 * finished, after btw saved the reply that asked for them, so only the reply that ends the
	 * turn has it.
	 */
	let usage: Usage | null = null;
	/** Live blocks, by the id of Codex's item. */
	const live = new Map<string, number>();
	let nextBlock = 0;
	/** Commands run one after another, as btw runs them. */
	let queue: Promise<unknown> = Promise.resolve();

	const block = (id: string, type: 'text' | 'thinking' | 'tool') => {
		const index = nextBlock++;
		live.set(id, index);
		turn.onEvent({ type: 'block_start', index, block: type === 'tool' ? { type, id } : { type } });
	};
	const delta = (id: unknown, text: unknown) => {
		const index = live.get(String(id));
		if (index !== undefined && typeof text === 'string' && text) {
			turn.onEvent({ type: 'delta', index, text });
		}
	};

	const following = follow((method, params) => {
		switch (method) {
			case 'item/started': {
				const item = params.item as { type: string; id: string; namespace?: string | null };
				if (item.type === 'agentMessage') block(item.id, 'text');
				else if (item.type === 'reasoning') block(item.id, 'thinking');
				else if (item.type === 'dynamicToolCall' && item.namespace === NAMESPACE) {
					block(item.id, 'tool');
				}
				return;
			}
			case 'item/agentMessage/delta':
			case 'item/reasoning/summaryTextDelta':
				return delta(params.itemId, params.delta);
			case 'item/reasoning/summaryPartAdded':
				if (Number(params.summaryIndex) > 0) delta(params.itemId, '\n\n');
				return;
			case 'item/completed': {
				const item = params.item as Record<string, unknown>;
				if (item.type === 'agentMessage' && typeof item.text === 'string' && item.text) {
					open.push({
						type: 'message',
						id: item.id,
						role: 'assistant',
						status: 'completed',
						content: [{ type: 'output_text', text: item.text, annotations: [] }],
						...(item.phase ? { phase: item.phase } : {})
					});
				} else if (item.type === 'reasoning') {
					const summary = (item.summary as string[] | undefined) ?? [];
					if (summary.some(Boolean)) {
						open.push({
							type: 'reasoning',
							id: item.id,
							summary: summary.map((text) => ({ type: 'summary_text', text }))
						});
					}
				}
				return;
			}
			case 'thread/tokenUsage/updated': {
				usage = usageOf((params.tokenUsage as { last: TokenUsage }).last);
				return;
			}
		}
	});

	/** Saves what the model said since the last reply, and the calls it asks for. */
	const saveReply = async (calls: ToolCall[]) => {
		const content: OutputItem[] = [
			...open,
			...calls.map((c) => ({
				type: 'function_call',
				call_id: c.id,
				name: c.name,
				arguments: JSON.stringify(c.input ?? {}),
				status: 'completed'
			}))
		];
		const reply: ModelReply = {
			content,
			stopReason: calls.length ? 'tool_use' : 'end_turn',
			usage: calls.length ? null : usage,
			calls,
			texts: textsOf(open)
		};
		open = [];
		live.clear();
		nextBlock = 0;
		await turn.onReply(reply);
	};

	/** Codex asks btw to run one of its tools. */
	const handleCall = (params: Record<string, unknown>) => {
		const name = String(params.tool);
		if (params.namespace !== NAMESPACE || !turn.tools.some((t) => t.name === name)) {
			return Promise.resolve({
				contentItems: [{ type: 'inputText', text: `btw has no tool ${name}.` }],
				success: false
			});
		}
		const call: ToolCall = {
			type: 'tool_call',
			id: String(params.callId),
			name,
			input: params.arguments ?? {}
		};
		const run = queue.then(async () => {
			if (turn.signal.aborted) throw new Error('the turn was stopped');
			await saveReply([call]);
			let result: ToolResultBlock;
			try {
				result = await turn.runTool(call);
			} catch (err) {
				const reason = err instanceof Error ? err.message : String(err);
				result = {
					type: 'tool_result',
					callId: call.id,
					content: `Not run: ${reason}`,
					isError: true
				};
			}
			turn.onResults([result]);
			return toToolResponse((await turn.resolve([result]))[0] as ToolResultBlock);
		});
		queue = run.catch(() => {});
		return run;
	};

	const codex = await openCodex({
		onNotification: following.onNotification,
		onRequest: async (method, params) => {
			if (method === 'item/tool/call') return handleCall(params);
			throw new AppServerError(`btw doesn't answer ${method}`, -32601);
		}
	});

	let threadId: string | null = null;
	let turnId: string | null = null;
	let closeTimer: NodeJS.Timeout | undefined;
	const onAbort = () => {
		if (threadId && turnId) {
			codex.request('turn/interrupt', { threadId, turnId }).catch(() => {});
		}
		closeTimer ??= setTimeout(() => codex.close(), INTERRUPT_GRACE_MS);
	};
	turn.signal.addEventListener('abort', onAbort, { once: true });

	let ended: { status: string; error: TurnError | null } | null = null;
	let thrown: unknown = null;
	try {
		const settings = {
			model: turn.model,
			cwd: turn.cwd,
			approvalPolicy: 'never',
			sandbox: 'read-only',
			baseInstructions: turn.system,
			serviceName: 'btw'
		};
		let thread: { thread: { id: string } };
		if (turn.resume) {
			try {
				thread = await codex.request('thread/resume', { threadId: turn.sessionId, ...settings });
			} catch (err) {
				if (
					err instanceof AppServerError &&
					/no rollout found|invalid session id/i.test(err.message)
				) {
					throw new PlanError(
						`Codex has no thread ${turn.sessionId} for this chat (${err.message}).`,
						'thread_not_found',
						{ cause: err }
					);
				}
				throw err;
			}
		} else {
			thread = await codex.request('thread/start', {
				...settings,
				dynamicTools: dynamicTools(turn.tools)
			});
		}
		threadId = thread.thread.id;
		const started = await codex.request<{ turn: { id: string } }>('turn/start', {
			threadId,
			input: toCodexInput(await turn.resolve(turn.input)),
			effort: await effortFor(codex, turn.model, turn.effort),
			summary: 'auto',
			// No computer of Codex's own to work on: btw's tools are the model's only way to act.
			environments: []
		});
		turnId = started.turn.id;
		turn.onStarted(threadId);
		if (turn.signal.aborted) onAbort();
		ended = await Promise.race([
			following.ended,
			codex.exited.then(() => {
				throw new AppServerExited('Codex ended in the middle of the turn.');
			})
		]);
	} catch (err) {
		thrown = err;
	} finally {
		turn.signal.removeEventListener('abort', onAbort);
		clearTimeout(closeTimer);
		// A command still running finishes (a stop ends it) and has its result saved.
		await queue;
		codex.close();
	}

	if (turn.signal.aborted) throw new PlanStopped();
	if (thrown) throw startError(thrown);
	if (ended?.status === 'failed') throw turnError(ended.error);
	if (ended?.status === 'interrupted') throw new PlanError('Codex stopped the turn.');
	// The reply that ends the turn. One cut off by a stop or a failure is dropped, as btw's own
	// loop drops it.
	if (open.length) await saveReply([]);
}

/**
 * A turn that failed because the chat's thread isn't what btw thought: `missing` when Codex no
 * longer has it (say its files were deleted). Codex picks its threads' ids, so none is ever
 * `taken`. Null for any other failure.
 */
export function sessionProblem(err: unknown): SessionProblem | null {
	return err instanceof PlanError && err.kind === 'thread_not_found' ? 'missing' : null;
}

// --- short exchanges ---

/** One short exchange without btw's tools or a saved thread, for chores like naming a chat. */
export async function quickReply(opts: {
	model: string;
	system: string;
	input: string;
	timeoutMs: number;
}): Promise<{ text: string | null; usage: Usage }> {
	let text: string | null = null;
	let usage: Usage = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 };
	const following = follow((method, params) => {
		if (method === 'item/completed') {
			const item = params.item as { type: string; text?: string };
			if (item.type === 'agentMessage' && item.text) text = item.text;
		} else if (method === 'thread/tokenUsage/updated') {
			usage = addUsage(usage, usageOf((params.tokenUsage as { last: TokenUsage }).last));
		}
	});
	let timer: NodeJS.Timeout | undefined;
	const ended = await withCodex(
		async (codex) => {
			const { thread } = await codex.request<{ thread: { id: string } }>('thread/start', {
				model: opts.model,
				cwd: paths.codexHome,
				approvalPolicy: 'never',
				sandbox: 'read-only',
				baseInstructions: opts.system,
				serviceName: 'btw',
				ephemeral: true
			});
			await codex.request('turn/start', {
				threadId: thread.id,
				input: [{ type: 'text', text: opts.input, text_elements: [] }],
				effort: 'low',
				summary: 'none',
				environments: []
			});
			return Promise.race([
				following.ended,
				new Promise<never>((_, reject) => {
					timer = setTimeout(
						() => reject(new PlanError('Codex took too long to answer.')),
						opts.timeoutMs
					);
				}),
				codex.exited.then(() => {
					throw new AppServerExited('Codex ended before it answered.');
				})
			]);
		},
		{ onNotification: following.onNotification }
	)
		.catch((err: unknown) => {
			throw startError(err);
		})
		.finally(() => clearTimeout(timer));
	if (ended.status === 'failed') throw turnError(ended.error);
	return { text: ended.status === 'completed' ? text : null, usage };
}

// --- models ---

/** A model Codex offers on the plan, as btw needs it. */
export interface ChatGptModel {
	id: string;
	name: string;
	description: string | null;
	/** Hidden models work but aren't offered in Codex's own model picker. */
	listed: boolean;
	isDefault: boolean;
	/** The reasoning efforts it takes, in Codex's words (`low`… `xhigh`, `max`). */
	efforts: string[];
}

async function readModels(codex: AppServer): Promise<ChatGptModel[]> {
	const models: ChatGptModel[] = [];
	let cursor: string | null = null;
	do {
		const page: { data: Record<string, unknown>[]; nextCursor: string | null } =
			await codex.request('model/list', { includeHidden: true, cursor });
		for (const m of page.data) {
			if (typeof m.id !== 'string' || !m.id) continue;
			models.push({
				id: m.id,
				name: typeof m.displayName === 'string' && m.displayName ? m.displayName : m.id,
				description: typeof m.description === 'string' && m.description ? m.description : null,
				listed: !m.hidden,
				isDefault: !!m.isDefault,
				efforts: Array.isArray(m.supportedReasoningEfforts)
					? (m.supportedReasoningEfforts as { reasoningEffort?: unknown }[]).flatMap((e) =>
							typeof e?.reasoningEffort === 'string' ? [e.reasoningEffort] : []
						)
					: []
			});
		}
		cursor = page.nextCursor;
	} while (cursor);
	return models;
}

/**
 * The chat's effort, or the nearest below it that the model takes, as Codex lists them: Codex
 * sends any effort on, which the model would refuse. As it is when Codex doesn't say.
 */
async function effortFor(codex: AppServer, model: string, effort: Effort): Promise<string> {
	let efforts: string[] = [];
	try {
		efforts = (await readModels(codex)).find((m) => m.id === model)?.efforts ?? [];
	} catch {
		// Codex checks the model itself when the turn starts.
	}
	if (!efforts.length || efforts.includes(effort)) return effort;
	const lower = EFFORTS.slice(0, EFFORTS.indexOf(effort)).reverse();
	return lower.find((e) => efforts.includes(e)) ?? efforts[0];
}

/** The models Codex offers on the plan. */
export function listChatGptModels(): Promise<ChatGptModel[]> {
	return withCodex(readModels).catch((err: unknown) => {
		throw startError(err);
	});
}

/** The models Codex's own picker offers, in its order, for the admin page's. */
export async function listModels(): Promise<ModelChoice[]> {
	return (await listChatGptModels()).flatMap((m) =>
		m.listed ? [{ id: m.id, name: m.name, description: m.description, contextWindow: null }] : []
	);
}

/**
 * Checks that Codex is signed in with ChatGPT and offers the model. Codex doesn't say a model's
 * context window, so it's null: Codex keeps each chat's thread within it itself.
 */
export async function fetchContextWindow(model: string): Promise<null> {
	await withCodex(async (codex) => {
		const problem = accountProblem(await readAccount(codex));
		if (problem) throw new PlanError(problem);
		const models = await readModels(codex);
		if (models.some((m) => m.id === model)) return;
		const offered = models.filter((m) => m.listed).map((m) => m.id);
		throw new PlanError(
			`Codex has no model "${model}" on the ChatGPT plan${offered.length ? `. It has ${offered.join(', ')}` : ''}.`,
			'not_found'
		);
	}).catch((err: unknown) => {
		throw startError(err);
	});
	return null;
}

// --- the sign-in ---

type Account =
	| { type: 'chatgpt'; email: string | null; planType: string | null }
	| { type: string; email?: undefined; planType?: undefined }
	| null;

async function readAccount(codex: AppServer): Promise<Account> {
	const { account } = await codex.request<{ account: Account }>('account/read', {
		refreshToken: false
	});
	return account ?? null;
}

const PLAN_NAMES: Record<string, string> = {
	free: 'ChatGPT Free',
	go: 'ChatGPT Go',
	plus: 'ChatGPT Plus',
	pro: 'ChatGPT Pro',
	prolite: 'ChatGPT Pro',
	team: 'ChatGPT Business',
	business: 'ChatGPT Business',
	enterprise: 'ChatGPT Enterprise',
	edu: 'ChatGPT Edu'
};

/** The plan as ChatGPT names it, from Codex's word for it ("plus", "self_serve_business_…"). */
function planName(planType: string | null | undefined): string | null {
	if (!planType || planType === 'unknown') return null;
	const exact = PLAN_NAMES[planType];
	if (exact) return exact;
	const family = Object.keys(PLAN_NAMES).find((key) => planType.split('_').includes(key));
	if (family) return PLAN_NAMES[family];
	if (/^ent/.test(planType)) return PLAN_NAMES.enterprise;
	return `ChatGPT (${planType})`;
}

function accountOf(account: Account): PlanAccount | null {
	if (account?.type !== 'chatgpt') return null;
	return { email: account.email ?? null, plan: planName(account.planType) };
}

/** What's wrong with how Codex is signed in, for chats on the plan. */
function accountProblem(account: Account): string | null {
	if (account?.type === 'chatgpt') return null;
	if (account?.type === 'apiKey') {
		return `Codex is signed in with an API key, which bills the OpenAI platform rather than a ChatGPT plan. ${CHATGPT_SIGN_IN_HELP}`;
	}
	if (account)
		return `Codex is signed in with ${account.type}, not a ChatGPT plan. ${CHATGPT_SIGN_IN_HELP}`;
	return `Codex isn't signed in with ChatGPT. ${CHATGPT_SIGN_IN_HELP}`;
}

export interface ChatGptSignIn {
	/** Where to enter the code, signed in to ChatGPT. */
	verificationUrl: string;
	userCode: string;
	/** When btw stops waiting for the code, in ms since the epoch. */
	expiresAt: number;
	/** Resolves once the code was entered and Codex saved the sign-in; rejects when it doesn't. */
	done: Promise<void>;
}

interface Attempt {
	signIn: ChatGptSignIn | null;
	/** Ends the sign-in, once: with an error unless it's done, said in `lastError` unless quiet. */
	finish: (err: PlanError | null, quiet?: boolean) => void;
}

/** The sign-in under way in this process: the Codex waiting for its code. */
let pending: Attempt | null = null;
/** Why the last sign-in in this process didn't finish. */
let lastError: string | null = null;

/**
 * Starts signing in with ChatGPT through Codex: Codex asks OpenAI for a one-time code and waits,
 * in the background, for it to be entered on any device. Replaces a sign-in already under way.
 * Codex saves the sign-in in its home; btw never sees it.
 */
export async function startChatGptSignIn(): Promise<ChatGptSignIn> {
	cancelChatGptSignIn();
	lastError = null;
	let settle!: { resolve: () => void; reject: (err: PlanError) => void };
	const done = new Promise<void>((resolve, reject) => (settle = { resolve, reject }));
	// Callers that never wait for it mustn't turn its rejection into an unhandled one.
	done.catch(() => {});

	let codex: AppServer | null = null;
	let loginId: string | null = null;
	let finished = false;
	const timer = setTimeout(
		() => attempt.finish(new PlanError('Nobody entered the sign-in code in time.')),
		SIGN_IN_TIMEOUT_MS
	);
	timer.unref();
	const attempt: Attempt = {
		signIn: null,
		finish: (err, quiet = false) => {
			if (finished) return;
			finished = true;
			clearTimeout(timer);
			if (pending === attempt) pending = null;
			const server = codex;
			if (err && loginId && server) {
				// Codex stops waiting, and the code goes unused.
				server
					.request('account/login/cancel', { loginId })
					.catch(() => {})
					.finally(() => server.close());
			} else server?.close();
			if (!err) return settle.resolve();
			if (!quiet) lastError = err.message;
			settle.reject(err);
		}
	};
	pending = attempt;
	try {
		codex = await openCodex({
			onNotification: (method, params) => {
				if (method !== 'account/login/completed' || params.loginId !== loginId) return;
				const why = params.error ? ` (${String(params.error)})` : '';
				attempt.finish(
					params.success ? null : new PlanError(`The sign-in with ChatGPT didn't finish${why}.`)
				);
			}
		});
		const login = await codex.request<{
			loginId: string;
			verificationUrl: string;
			userCode: string;
		}>('account/login/start', { type: 'chatgptDeviceCode' });
		loginId = login.loginId;
		attempt.signIn = {
			verificationUrl: login.verificationUrl,
			userCode: login.userCode,
			expiresAt: Date.now() + SIGN_IN_TIMEOUT_MS,
			done
		};
	} catch (err) {
		const error = startError(err);
		attempt.finish(error);
		throw error;
	}
	if (finished) throw new PlanError('The sign-in was cancelled.');
	void codex.exited.then(() =>
		attempt.finish(new PlanError('Codex stopped before the sign-in finished.'))
	);
	return attempt.signIn;
}

/** Stops waiting for the code of a sign-in under way. The code then goes unused. */
export function cancelChatGptSignIn(): void {
	pending?.finish(new PlanError('The sign-in was cancelled.'), true);
}

/** The sign-in under way in this process, if any, and why the last one didn't finish. */
export function chatGptSignInState(): {
	pending: Omit<ChatGptSignIn, 'done'> | null;
	signInError: string | null;
} {
	const signIn = pending?.signIn;
	return {
		pending: signIn
			? {
					verificationUrl: signIn.verificationUrl,
					userCode: signIn.userCode,
					expiresAt: signIn.expiresAt
				}
			: null,
		signInError: lastError
	};
}

/** Signs Codex out: it forgets the sign-in and asks OpenAI to revoke it. */
export async function signOutChatGpt(): Promise<void> {
	cancelChatGptSignIn();
	lastError = null;
	await withCodex((codex) => codex.request('account/logout')).catch((err: unknown) => {
		throw startError(err);
	});
}

export interface ChatGptPlanStatus extends PlanStatus {
	account: PlanAccount | null;
}

/**
 * Starts Codex and asks who it's signed in as. Nothing is billed. For Models & keys, `btw
 * chatgpt-plan status` and adding a preset.
 */
export async function chatGptPlanStatus(): Promise<ChatGptPlanStatus> {
	const { path, installed } = findCodex();
	let timer: NodeJS.Timeout | undefined;
	try {
		const account = await withCodex((codex) =>
			Promise.race([
				readAccount(codex),
				new Promise<never>((_, reject) => {
					timer = setTimeout(() => reject(new Error('Codex didn’t answer.')), STATUS_TIMEOUT_MS);
				})
			])
		);
		const plan = accountOf(account);
		return {
			path,
			installed,
			account: plan,
			signedIn: plan && describePlanAccount(plan),
			problem: accountProblem(account)
		};
	} catch (err) {
		return { path, installed, account: null, signedIn: null, problem: startError(err).message };
	} finally {
		clearTimeout(timer);
	}
}

/** Throws a PlanError unless Codex is here and signed in with ChatGPT. */
export async function checkChatGptPlan(): Promise<ChatGptPlanStatus> {
	const status = await chatGptPlanStatus();
	if (status.problem) throw new PlanError(status.problem);
	return status;
}
