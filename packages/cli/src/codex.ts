import {
	cancelCodexSignIn,
	codexAccount,
	describeApiError,
	listCodexModels,
	signOutCodex,
	startCodexSignIn,
	type CodexAccount
} from '@btw/core';
import type { Io } from './io.ts';

export const CODEX_HELP = `ChatGPT plan (provider codex: OpenAI's models on a Plus, Pro or Business plan, as in Codex)
  btw codex login                            sign in with ChatGPT: open the link on any device and
                                             enter the code it prints. Admins can also do this on
                                             the web, under Models & keys
  btw codex logout                           sign out; chats on codex presets stop until someone
                                             signs in again
  btw codex status                           who btw is signed in as
  btw codex models                           the models the plan offers, for \`btw preset add\``;

export function describeAccount(account: CodexAccount): string {
	const plan = account.plan ? ` (${account.plan} plan)` : '';
	return `${account.email ?? 'a ChatGPT account'}${plan}`;
}

/**
 * Signs in with ChatGPT's device code: prints where to go and the code, then waits for someone
 * to enter it. Stops waiting when the command is stopped.
 */
export async function signInWithChatGpt(io: Io): Promise<CodexAccount> {
	const signIn = await startCodexSignIn();
	const stop = () => cancelCodexSignIn();
	io.signal.addEventListener('abort', stop, { once: true });
	io.log(`To sign in with ChatGPT, on any device:
  1. Open ${signIn.verificationUrl} and sign in to ChatGPT
  2. Enter this code: ${signIn.userCode}
The code works for 15 minutes. Enter it only if you started this sign-in. Waiting…`);
	try {
		const account = await signIn.done;
		io.log(`Signed in as ${describeAccount(account)}.`);
		return account;
	} finally {
		io.signal.removeEventListener('abort', stop);
	}
}

export async function codexCommand(io: Io, action: string | undefined): Promise<void> {
	switch (action) {
		case 'login':
			await signInWithChatGpt(io);
			return;
		case 'logout': {
			const account = codexAccount();
			await signOutCodex();
			io.log(
				account
					? `Signed out of ${describeAccount(account)}. Chats on codex presets stop until someone signs in again.`
					: 'Not signed in.'
			);
			return;
		}
		case 'status':
		case undefined: {
			const account = codexAccount();
			io.log(
				account
					? `Signed in as ${describeAccount(account)}.`
					: 'Not signed in. Run `btw codex login`.'
			);
			return;
		}
		case 'models': {
			let models;
			try {
				models = await listCodexModels();
			} catch (err) {
				throw new Error(describeApiError(err, 'codex'), { cause: err });
			}
			for (const m of models.filter((m) => m.listed)) {
				const context = m.contextWindow ? `\tcontext ${Math.round(m.contextWindow / 1000)}K` : '';
				io.log(`${m.id}\t${m.name}${context}`);
			}
			return;
		}
		case 'help':
			io.log(CODEX_HELP);
			return;
		default:
			throw new Error(`unknown codex command "${action}". See \`btw codex help\`.`);
	}
}
