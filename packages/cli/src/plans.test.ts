import { initConfig, startChatGptSignIn } from '@nolune/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { codexRequests, useFakeCodex } from '../../core/src/test/fake-codex.ts';
import { runCli } from './run.ts';
import { testIo } from './test/io.ts';

/*
 * `nolune chatgpt-plan setup` with a stand-in for Codex's app server. Run by the gateway, the command
 * is stopped through its signal when the client hangs up (Ctrl-C), and the sign-in with it.
 */

vi.mock('@nolune/core', async (importOriginal) => {
	const core = await importOriginal<typeof import('@nolune/core')>();
	return { ...core, startChatGptSignIn: vi.fn(core.startChatGptSignIn) };
});

beforeEach(() => {
	initConfig();
	useFakeCodex();
});

afterEach(() => {
	vi.clearAllMocks();
});

describe('nolune chatgpt-plan setup', { timeout: 20_000 }, () => {
	it('cancels the sign-in when stopped while Codex starts', async () => {
		const abort = new AbortController();
		const start = vi.mocked(startChatGptSignIn);
		const startForReal = start.getMockImplementation()!;
		start.mockImplementationOnce(() => {
			const starting = startForReal();
			abort.abort();
			return starting;
		});
		const { io, out, err } = testIo({ stdin: '', signal: abort.signal });

		expect(await runCli(['chatgpt-plan', 'setup'], io)).toBe(1);
		expect(out()).toBe("Codex isn't signed in with ChatGPT.\n");
		expect(err()).toBe('nolune: The sign-in was cancelled.\n');
		// The code Codex asked for goes unused.
		expect(codexRequests().map((r) => r.method)).toContain('account/login/cancel');
	});

	it("doesn't start a sign-in when stopped before it", async () => {
		const { io, err } = testIo({ stdin: '', signal: AbortSignal.abort() });

		expect(await runCli(['chatgpt-plan', 'setup'], io)).toBe(1);
		expect(err()).toBe('nolune: This operation was aborted\n');
		expect(startChatGptSignIn).not.toHaveBeenCalled();
	});
});
