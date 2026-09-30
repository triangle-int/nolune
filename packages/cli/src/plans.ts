import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import {
	CLAUDE_INSTALL_COMMAND,
	cancelChatGptSignIn,
	chatGptPlanStatus,
	chatGptSignInState,
	claudePlanStatus,
	claudeSignInCommand,
	finishChatGptSignIn,
	listChatGptModels,
	signOutChatGpt,
	startChatGptSignIn,
	type Plan
} from '@nolune/core';
import { ask } from './input.ts';
import { fail, type Io } from './io.ts';

/*
 * `nolune claude-plan` and `nolune chatgpt-plan`. The Claude plan runs Claude Code, which keeps
 * the sign-in: these check it, and offer to install Claude Code and sign it in where needed. The
 * ChatGPT plan needs nothing installed: these sign in with ChatGPT, which nolune keeps.
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

/** Opens `url` in this computer's browser, where there's one to open; it's printed either way. */
function openBrowser(url: string): void {
	const command =
		process.platform === 'darwin' ? 'open' : process.platform === 'linux' ? 'xdg-open' : null;
	if (!command) return;
	try {
		const child = spawn(command, [url], { stdio: 'ignore', detached: true });
		child.on('error', () => {});
		child.unref();
	} catch {
		// no browser here
	}
}

/** At a terminal, takes the address a sign-in ended on, pasted, until the sign-in is over. */
async function takePasted(io: Io, signal: AbortSignal): Promise<void> {
	const rl = createInterface({ input: process.stdin, output: process.stdout });
	try {
		while (!signal.aborted) {
			const address = (await rl.question('', { signal })).trim();
			if (!address) continue;
			try {
				await finishChatGptSignIn(address);
				return;
			} catch (err) {
				// When the sign-in ended with it, that's said once, by the command.
				if (chatGptSignInState().pending) io.log((err as Error).message);
			}
		}
	} catch {
		// the sign-in is over
	} finally {
		rl.close();
	}
}

/**
 * Signs in with ChatGPT: prints OpenAI's sign-in page, opens it in this computer's browser at a
 * terminal, and waits for the browser to come back. A browser on another device ends on an address
 * that doesn't load, which can be pasted here at a terminal, or else under Models & keys, where
 * this sign-in shows too. Stops when the command is stopped.
 */
async function signInWithChatGpt(io: Io, anotherAccount: boolean): Promise<void> {
	// Stopped already: no sign-in, and the one under way (the admin page's, say) isn't ours to cancel.
	io.signal.throwIfAborted();
	const stop = () => cancelChatGptSignIn();
	io.signal.addEventListener('abort', stop, { once: true });
	const reading = new AbortController();
	try {
		const signIn = await startChatGptSignIn({ anotherAccount });
		io.log(`To sign in with ChatGPT, open this in a browser and allow nolune to use your plan:

  ${signIn.url}
`);
		if (io.stdinIsTTY) {
			openBrowser(signIn.url);
			io.log(
				"In a browser on another device, the page ChatGPT sends you back to won't load: copy its address and paste it here. Waiting…"
			);
			void takePasted(io, reading.signal);
		} else {
			io.log(
				"Open it in a browser on this computer. On another device, the page ChatGPT sends you back to won't load: paste its address under Models & keys on nolune's admin page, where this sign-in shows too. Waiting…"
			);
		}
		await signIn.done;
	} finally {
		reading.abort();
		io.signal.removeEventListener('abort', stop);
	}
}

/**
 * Fails unless someone is signed in with ChatGPT and OpenAI takes the sign-in; says who. With
 * `guide`, it first signs in when that's missing or doesn't work, or with `anotherAccount`.
 */
export async function requireChatGptPlan(
	io: Io,
	opts: { guide?: boolean; anotherAccount?: boolean } = {}
): Promise<void> {
	let status = await chatGptPlanStatus({ check: true });
	if (opts.guide && (opts.anotherAccount || status.problem)) {
		if (status.problem) io.log(status.problem.split('. ')[0].replace(/\.?$/, '.'));
		await signInWithChatGpt(io, !!opts.anotherAccount);
		status = await chatGptPlanStatus({ check: true });
	}
	if (status.problem || !status.signedIn)
		fail(status.problem ?? 'Nobody is signed in with ChatGPT.');
	io.log(`ChatGPT plan: ${status.signedIn}`);
}

/**
 * `nolune <plan> setup`: installs the plan's agent and signs it in where needed, then says who it's
 * signed in as.
 */
function setUpPlan(io: Io, plan: Plan, args: string[]): Promise<void> {
	if (plan === 'claude-plan') return requireClaudePlan(io, true);
	const anotherAccount = args.includes('--another-account');
	const unknown = args.find((arg) => arg !== '--another-account');
	if (unknown)
		fail(`unknown option ${unknown}. usage: nolune chatgpt-plan setup [--another-account]`);
	return requireChatGptPlan(io, { guide: true, anotherAccount });
}

/** `nolune claude-plan …` and `nolune chatgpt-plan …`. */
export async function planCommand(
	io: Io,
	plan: Plan,
	action: string | undefined,
	args: string[] = []
): Promise<void> {
	switch (action) {
		case 'status':
			return plan === 'claude-plan' ? requireClaudePlan(io) : requireChatGptPlan(io);
		case 'setup':
			// Claude Code signs in on the terminal; ChatGPT's sign-in is a page in a browser.
			if (plan === 'claude-plan' && !io.stdinIsTTY) {
				fail('`nolune claude-plan setup` asks questions: run it in a terminal on this computer.');
			}
			return setUpPlan(io, plan, args);
	}
	if (plan === 'chatgpt-plan') {
		if (action === 'logout') {
			const { signedIn } = await chatGptPlanStatus();
			const told = await signOutChatGpt();
			io.log(
				signedIn
					? `Signed out (was ${signedIn}).${told ? '' : " OpenAI couldn't be told: to be sure, disconnect nolune in ChatGPT's settings."} Chats on chatgpt-plan presets stop until someone signs in again.`
					: 'Not signed in.'
			);
			return;
		}
		if (action === 'models') {
			for (const m of await listChatGptModels()) {
				if (m.listed) io.log(`${m.id}\t${m.name}`);
			}
			return;
		}
		fail('usage: nolune chatgpt-plan status|setup [--another-account]|logout|models');
	}
	fail('usage: nolune claude-plan status|setup');
}
