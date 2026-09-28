import type Anthropic from '@anthropic-ai/sdk';
import type { Block, ToolResultBlock } from './format.ts';
import type { Effort, ModelReply, StreamEvent, ToolCall } from './models.ts';

/*
 * Chats on someone's subscription plan instead of an API key: the Claude plan (claude-plan.ts,
 * through Claude Code) and the ChatGPT plan (chatgpt-plan.ts, through Codex). Both run the maker's
 * own agent, installed on this computer and signed in there: it keeps the sign-in, the
 * conversation and the agent loop, and asks nolune to run each command. So both take the same whole
 * turn from the runner (PlanTurn), and what people see of them is shared: one kind of error, one
 * way of saying who a plan is signed in as, and one status for Models & keys, `nolune <plan> status`
 * and adding a preset.
 *
 * This module imports neither plan, so both can build on it.
 */

export const PLANS = ['claude-plan', 'chatgpt-plan'] as const;
export type Plan = (typeof PLANS)[number];

export function isPlan(provider: string): provider is Plan {
	return (PLANS as readonly string[]).includes(provider);
}

/** What went wrong with a plan, in words for the people in the chat. */
export class PlanError extends Error {
	/** The plan's own kind of error (`authentication_failed`, `usageLimitExceeded`...), when it said. */
	readonly kind: string | null;
	constructor(message: string, kind: string | null = null, options?: ErrorOptions) {
		super(message, options);
		this.kind = kind;
	}
}

/** A plan's turn that ended because someone stopped it. */
export class PlanStopped extends Error {
	constructor() {
		super('Stopped');
	}
}

export function isPlanStopped(err: unknown): boolean {
	return err instanceof PlanStopped;
}

/** One turn of a chat on a plan, as the runner hands it over. */
export interface PlanTurn {
	/**
	 * The chat's session in the plan's agent, and whether it exists yet. A new one is created with
	 * this id where the agent takes one (Claude Code); Codex picks its own (`onStarted` says it).
	 */
	sessionId: string;
	resume: boolean;
	/** The agent's working folder: the profile's. */
	cwd: string;
	model: string;
	effort: Effort;
	system: string;
	tools: Anthropic.Tool[];
	/** What the model hasn't seen yet, sent as one message. */
	input: Block[];
	/** Pictures and PDFs kept by reference, with their bytes: the agent gets them inline. */
	resolve: (blocks: Block[]) => Promise<Block[]>;
	signal: AbortSignal;
	/** The agent took the input into this session: from here on it's there, even if the turn fails. */
	onStarted: (sessionId: string) => void;
	onEvent: (event: StreamEvent) => void;
	/** A reply to save. Its commands wait until this resolves. */
	onReply: (reply: ModelReply) => Promise<void>;
	/** Runs a call of the reply that was saved last. */
	runTool: (call: ToolCall) => Promise<ToolResultBlock>;
	/** Every call of the reply that was saved last has its result, in the reply's order. */
	onResults: (results: ToolResultBlock[]) => void;
}

/**
 * Why a turn failed on the chat's session rather than anything else: `missing` (the agent no
 * longer has it, say its files were deleted) or `taken` (it exists, though nolune never saw it start).
 */
export type SessionProblem = 'missing' | 'taken';

/** Who a plan is signed in as. */
export interface PlanAccount {
	email: string | null;
	/** The plan as its maker names it: "Claude Max", "ChatGPT Plus". */
	plan: string | null;
}

/** "signed in as anna@example.com (Claude Max)". */
export function describePlanAccount(account: PlanAccount): string {
	if (account.email)
		return `signed in as ${account.email}${account.plan ? ` (${account.plan})` : ''}`;
	return account.plan ? `signed in to ${account.plan}` : 'signed in';
}

export interface PlanStatus {
	/** The agent nolune runs (`claude`, `codex`), if it found one or was told where it is. */
	path: string | null;
	/** Whether that agent is there. */
	installed: boolean;
	/** Who the plan is signed in as ("signed in as …"); null when it isn't, or couldn't be asked. */
	signedIn: string | null;
	/** What stops chats on the plan from working, in plain words; null when nothing does. */
	problem: string | null;
}
