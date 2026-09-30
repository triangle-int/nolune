import { chatGptSignInState, initConfig, startChatGptSignIn } from '@nolune/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { runCli } from './run.ts';
import { testIo } from './test/io.ts';

/*
 * `nolune chatgpt-plan setup`. Run by the gateway, the command is stopped through its signal when
 * the client hangs up (Ctrl-C), and the sign-in with it.
 */

vi.mock('@nolune/core', async (importOriginal) => {
	const core = await importOriginal<typeof import('@nolune/core')>();
	return { ...core, startChatGptSignIn: vi.fn(core.startChatGptSignIn) };
});

beforeEach(() => {
	initConfig();
});

afterEach(() => {
	vi.clearAllMocks();
});

describe('nolune chatgpt-plan setup', () => {
	it('prints the sign-in page, and cancels the sign-in when stopped while waiting', async () => {
		const abort = new AbortController();
		const { io, out, err } = testIo({ stdin: '', signal: abort.signal });

		const running = runCli(['chatgpt-plan', 'setup'], io);
		await vi.waitFor(() => expect(chatGptSignInState().pending).not.toBeNull());
		abort.abort();

		expect(await running).toBe(1);
		expect(out()).toContain('Nobody is signed in with ChatGPT.');
		expect(out()).toContain(
			'https://auth.openai.com/api/accounts/authorize?client_id=dynamic_agent_client'
		);
		expect(out()).toContain('paste its address under Models & keys');
		expect(err()).toBe('nolune: The sign-in was cancelled.\n');
		expect(chatGptSignInState().pending).toBeNull();
	});

	it("doesn't start a sign-in when stopped before it", async () => {
		const { io, err } = testIo({ stdin: '', signal: AbortSignal.abort() });

		expect(await runCli(['chatgpt-plan', 'setup'], io)).toBe(1);
		expect(err()).toBe('nolune: This operation was aborted\n');
		expect(startChatGptSignIn).not.toHaveBeenCalled();
	});

	it('refuses an option it does not know', async () => {
		const { io, err } = testIo({ stdin: '' });

		expect(await runCli(['chatgpt-plan', 'setup', '--codex'], io)).toBe(1);
		expect(err()).toContain('unknown option --codex');
		expect(startChatGptSignIn).not.toHaveBeenCalled();
	});
});
