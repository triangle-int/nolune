import { spawnSync } from 'node:child_process';
import {
	CLAUDE_INSTALL_COMMAND,
	CODEX_INSTALL_COMMAND,
	cancelChatGptSignIn,
	chatGptPlanStatus,
	claudePlanStatus,
	claudeSignInCommand,
	listChatGptModels,
	signOutChatGpt,
	startChatGptSignIn,
	type Plan
} from '@nolune/core';
import { ask } from './input.ts';
import { fail, type Io } from './io.ts';

/*
 * `nolune claude-plan` and `nolune chatgpt-plan`. Each plan runs its maker's agent (Claude Code,
 * Codex), which keeps the sign-in: these check it, and offer to install the agent and sign it in
 * where needed.
 */

async function confirm(io: Io, question: string): Promise<boolean> {
	return /^y/i.test(await ask(io, `${question} (yes/no)`, 'yes'));
}

/** Runs a program on this terminal, for the person at it; returns its exit code. */
function runOnTerminal(
	command: string,
	args: string[],
	env: Record<string, string | undefined> = process.env
): number {
	return spawnSync(command, args, { stdio: 'inherit', env }).status ?? 1;
}

/**
 * Fails unless Claude Code is here and signed in to a plan; says who it's signed in as. With
 * `guide`, at a terminal, it first offers what's missing: Anthropic's installer, then Claude
 * Code's own sign-in, asking before each. nolune never sees the sign-in: Claude Code keeps it.
 */
export async function requireClaudePlan(io: Io, guide = false): Promise<void> {
	let status = await claudePlanStatus();
	if (guide && io.stdinIsTTY && !status.installed) {
		io.log("Chats on the Claude plan run through Claude Code, which isn't installed here.");
		if (await confirm(io, `Install it with Anthropic's installer (${CLAUDE_INSTALL_COMMAND})?`)) {
			if (runOnTerminal('bash', ['-c', CLAUDE_INSTALL_COMMAND]) !== 0) {
				fail("Claude Code's installer failed. See https://code.claude.com/docs/en/setup");
			}
			status = await claudePlanStatus();
		}
	}
	if (guide && io.stdinIsTTY && status.installed && status.account && status.problem) {
		// Its first sentence: the rest says how to sign in, which is what comes next.
		io.log(status.problem.split('. ')[0].replace(/\.?$/, '.'));
		if (await confirm(io, 'Sign in to your Claude plan now? Claude Code opens its sign-in page')) {
			const { command, args, env } = claudeSignInCommand(status.path ?? 'claude');
			runOnTerminal(command, args, env);
			status = await claudePlanStatus();
		}
	}
	if (status.problem || !status.account) fail(status.problem ?? "Claude Code didn't answer.");
	io.log(`Claude Code (${status.path}): ${status.signedIn}`);
}

/**
 * Signs Codex in with ChatGPT: prints where to go and a one-time code, then waits for someone to
 * enter it, on any device. It asks nothing here, so it also works from the agent's commands.
 * Stops when the command is stopped, even while Codex is still starting.
 */
async function signInWithChatGpt(io: Io): Promise<void> {
	// Stopped already: no sign-in, and the one under way (the admin page's, say) isn't ours to cancel.
	io.signal.throwIfAborted();
	const stop = () => cancelChatGptSignIn();
	io.signal.addEventListener('abort', stop, { once: true });
	try {
		const signIn = await startChatGptSignIn();
		io.log(`To sign in with ChatGPT, on any device:
  1. Open ${signIn.verificationUrl} and sign in to ChatGPT
  2. Enter this code: ${signIn.userCode}
The code works for 15 minutes. Enter it only if you started this sign-in. Waiting…`);
		await signIn.done;
	} finally {
		io.signal.removeEventListener('abort', stop);
	}
}

/**
 * Fails unless Codex is here and signed in with ChatGPT; says who it's signed in as. With
 * `guide`, it first does what's missing: at a terminal, it offers OpenAI's installer, asking
 * first; then it signs in with ChatGPT. nolune never sees the sign-in: Codex keeps it.
 */
export async function requireChatGptPlan(io: Io, guide = false): Promise<void> {
	let status = await chatGptPlanStatus();
	if (guide && io.stdinIsTTY && !status.installed) {
		io.log("Chats on the ChatGPT plan run through OpenAI's Codex, which isn't installed here.");
		if (await confirm(io, `Install it with npm (${CODEX_INSTALL_COMMAND})?`)) {
			if (runOnTerminal('bash', ['-c', CODEX_INSTALL_COMMAND]) !== 0) {
				fail("Codex's installer failed. See https://developers.openai.com/codex/cli");
			}
			status = await chatGptPlanStatus();
		}
	}
	if (guide && status.installed && !status.account) {
		if (status.problem) io.log(status.problem.split('. ')[0].replace(/\.?$/, '.'));
		await signInWithChatGpt(io);
		status = await chatGptPlanStatus();
	}
	if (status.problem || !status.signedIn) fail(status.problem ?? "Codex didn't answer.");
	io.log(`Codex (${status.path}): ${status.signedIn}`);
}

/**
 * `nolune <plan> setup`: installs the plan's agent and signs it in where needed, then says who it's
 * signed in as.
 */
function setUpPlan(io: Io, plan: Plan): Promise<void> {
	return plan === 'claude-plan' ? requireClaudePlan(io, true) : requireChatGptPlan(io, true);
}

/** `nolune claude-plan …` and `nolune chatgpt-plan …`. */
export async function planCommand(io: Io, plan: Plan, action: string | undefined): Promise<void> {
	switch (action) {
		case 'status':
			return plan === 'claude-plan' ? requireClaudePlan(io) : requireChatGptPlan(io);
		case 'setup':
			// Claude Code signs in on the terminal; Codex's sign-in is a link and a code, anywhere.
			if (plan === 'claude-plan' && !io.stdinIsTTY) {
				fail('`nolune claude-plan setup` asks questions: run it in a terminal on this computer.');
			}
			return setUpPlan(io, plan);
	}
	if (plan === 'chatgpt-plan') {
		if (action === 'logout') {
			const { signedIn } = await chatGptPlanStatus();
			await signOutChatGpt();
			io.log(
				signedIn
					? `Signed out (was ${signedIn}). Chats on chatgpt-plan presets stop until someone signs in again.`
					: 'Not signed in.'
			);
			return;
		}
		if (action === 'models') {
			for (const m of await listChatGptModels()) {
				if (m.listed) io.log(`${m.id}\t${m.name}${m.isDefault ? '\t(default)' : ''}`);
			}
			return;
		}
		fail('usage: nolune chatgpt-plan status|setup|logout|models');
	}
	fail('usage: nolune claude-plan status|setup');
}
