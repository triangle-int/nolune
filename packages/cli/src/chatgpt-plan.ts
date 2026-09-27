import {
	cancelChatGptSignIn,
	chatGptPlanStatus,
	listChatGptModels,
	signOutChatGpt,
	startChatGptSignIn
} from '@btw/core';
import { fail, type Io } from './io.ts';

/*
 * The ChatGPT plan's side of `btw <plan> status|setup` (the Claude plan's is requireClaudePlan in
 * run.ts), and what only it has: signing out, since btw holds this sign-in, and its models.
 */

/** Says who the plan is signed in as, the way `btw claude-plan status` does; fails when it isn't. */
function report(io: Io): void {
	const status = chatGptPlanStatus();
	if (!status.signedIn) fail(status.problem ?? 'Not signed in with ChatGPT.');
	io.log(`ChatGPT: ${status.signedIn}`);
}

/**
 * Signs in with ChatGPT where needed: prints where to go and a one-time code, then waits for
 * someone to enter it, on any device. Unlike the Claude plan's, it asks nothing here, so it also
 * works from the agent's commands. Stops waiting when the command is stopped.
 */
export async function setUpChatGptPlan(io: Io): Promise<void> {
	if (!chatGptPlanStatus().signedIn) {
		const signIn = await startChatGptSignIn();
		const stop = () => cancelChatGptSignIn();
		io.signal.addEventListener('abort', stop, { once: true });
		io.log(`To sign in with ChatGPT, on any device:
  1. Open ${signIn.verificationUrl} and sign in to ChatGPT
  2. Enter this code: ${signIn.userCode}
The code works for 15 minutes. Enter it only if you started this sign-in. Waiting…`);
		try {
			await signIn.done;
		} finally {
			io.signal.removeEventListener('abort', stop);
		}
	}
	report(io);
}

export async function chatGptPlanCommand(io: Io, action: string | undefined): Promise<void> {
	switch (action) {
		case 'status':
			return report(io);
		case 'setup':
			return setUpChatGptPlan(io);
		case 'logout': {
			const { signedIn } = chatGptPlanStatus();
			await signOutChatGpt();
			io.log(
				signedIn
					? `Signed out (was ${signedIn}). Chats on chatgpt-plan presets stop until someone signs in again.`
					: 'Not signed in.'
			);
			return;
		}
		case 'models':
			for (const m of (await listChatGptModels()).filter((m) => m.listed)) {
				const context = m.contextWindow ? `\tcontext ${Math.round(m.contextWindow / 1000)}K` : '';
				io.log(`${m.id}\t${m.name}${context}`);
			}
			return;
		default:
			fail('usage: btw chatgpt-plan status|setup|logout|models');
	}
}
