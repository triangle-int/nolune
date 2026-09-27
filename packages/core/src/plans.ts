/*
 * Chats on someone's subscription plan instead of an API key: the Claude plan (claude-plan.ts,
 * through Claude Code) and the ChatGPT plan (chatgpt-plan.ts, through Codex's backend). How each
 * one runs is its own. What people see of them is shared: one kind of error, one way of saying
 * who a plan is signed in as, and one status for Models & keys, `btw <plan> status` and adding a
 * preset.
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
	/** The plan's own kind of error (`authentication_failed`, `usage_limit_reached`...), when it said. */
	readonly kind: string | null;
	constructor(message: string, kind: string | null = null, options?: ErrorOptions) {
		super(message, options);
		this.kind = kind;
	}
}

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
	/** Who the plan is signed in as ("signed in as …"); null when it isn't, or couldn't be asked. */
	signedIn: string | null;
	/** What stops chats on the plan from working, in plain words; null when nothing does. */
	problem: string | null;
}
