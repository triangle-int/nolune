import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
	createConversation,
	createProfile,
	initConfig,
	readMemoryNote,
	readSoulFile,
	runSubagent
} from '@nolune/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setSubagentStatus } from '../../core/src/subagents.ts';
import { makeFamily, makePreset, makeUser } from '../../core/src/test/fixtures.ts';
import { runCli } from './run.ts';
import { testIo } from './test/io.ts';

/*
 * `nolune` as a function: everything a command reads and writes besides its arguments and nolune's
 * files goes through the io it's given, never the process's own, so it can run on someone
 * else's behalf inside another process.
 */

/** A 1×1 PNG. */
const DOT = Buffer.from(
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
	'base64'
);

beforeEach(() => {
	// As after `nolune setup`: commands that need it refuse to run without a config.
	initConfig();
});

afterEach(() => {
	vi.restoreAllMocks();
});

async function run(argv: string[], opts: Parameters<typeof testIo>[0] = {}) {
	const { io, out, err } = testIo(opts);
	const code = await runCli(argv, io);
	return { code, out: out(), err: err() };
}

describe('runCli', () => {
	it("uses the environment it's given, not the process's", async () => {
		createProfile('Family', makeUser('Anna').id);
		createProfile('Work', makeUser('Max').id);
		vi.stubEnv('NOLUNE_PROFILE', 'work');

		const added = await run(['memory', 'add', 'family', 'Anna is 7'], {
			env: { NOLUNE_PROFILE: 'family' }
		});

		expect(added).toEqual({ code: 0, out: 'Started family.md.\n', err: '' });
		expect(readMemoryNote('family', 'family').text).toContain('Anna is 7');
		expect(() => readMemoryNote('work', 'family')).toThrow();
		vi.unstubAllEnvs();
	});

	it("reads what's piped in from its stdin, and refuses a terminal with nothing piped in", async () => {
		createProfile('Family', makeUser('Anna').id);
		const env = { NOLUNE_PROFILE: 'family' };

		expect(
			await run(['memory', 'write', 'people/anna'], { env, stdin: '- Loves drawing\n' })
		).toEqual({ code: 0, out: 'Created people/anna.md.\n', err: '' });
		expect(readMemoryNote('family', 'people/anna').text).toBe('- Loves drawing\n');
		expect(await run(['soul', 'write'], { env, stdin: 'Warm and brief.' })).toMatchObject({
			code: 0
		});
		expect(readSoulFile('family')).toBe('Warm and brief.');

		expect(await run(['memory', 'write', 'people/max'], { env })).toEqual({
			code: 1,
			out: '',
			err: 'nolune: give the note as text, or pipe it in: nolune memory write <topic> < note.md\n'
		});
	});

	it('resolves relative paths from its folder', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'nolune-cwd-'));
		writeFileSync(join(cwd, 'dot.png'), DOT);
		const view = mkdtempSync(join(tmpdir(), 'nolune-view-'));
		writeFileSync(join(view, 'limits.json'), JSON.stringify({ count: 5 }));

		// The picture keeps the name as typed; the gateway reads it from the manifest.
		expect(await run(['view', 'dot.png'], { cwd, env: { NOLUNE_VIEW_DIR: view } })).toEqual({
			code: 0,
			out: 'Attached dot.png (1×1 PNG).\n',
			err: ''
		});
		expect(readFileSync(join(view, 'manifest.jsonl'), 'utf8')).toContain('"name":"dot.png"');

		const dry = await run(
			['generate', 'image', 'a cat', '--image', 'dot.png', '--out', 'pics', '--dry-run'],
			{ cwd }
		);
		expect(dry.out).toContain(`Images: ${join(cwd, 'dot.png')}\n`);
	});

	it('ends in an exit code and a message, never by exiting the process', async () => {
		const exit = vi.spyOn(process, 'exit').mockImplementation(() => {
			throw new Error('process.exit was called');
		});
		const exitCode = process.exitCode;

		expect(await run(['frobnicate'])).toEqual({
			code: 1,
			out: '',
			err: 'nolune: unknown command "frobnicate". See `nolune help`.\n'
		});
		expect(await run(['view', 'dot.png'])).toMatchObject({
			code: 1,
			err: "nolune: `nolune view` only works in the agent's commands: it shows images to the agent.\n"
		});
		const missing = await run(['view', 'nope.png'], {
			env: { NOLUNE_VIEW_DIR: mkdtempSync(join(tmpdir(), 'nolune-view-')) }
		});
		expect(missing.code).toBe(1);

		expect(exit).not.toHaveBeenCalled();
		expect(process.exitCode).toBe(exitCode);
	});

	it('prints the help without arguments', async () => {
		const help = await run([]);
		expect(help.code).toBe(0);
		expect(help.out).toMatch(/^nolune - a family agent that runs on this computer\n/);
	});
});

describe('nolune preset edit', () => {
	it("changes what it's given, and says chats on it keep theirs", async () => {
		makePreset('Sonnet');
		expect(
			await run(['preset', 'edit', 'Sonnet', '--name', 'Everyday', '--context-window', '272000'])
		).toEqual({
			code: 0,
			out: 'Saved "Everyday" (anthropic/claude-sonnet-5, context 272K). Chats already on it keep what they had.\n',
			err: ''
		});
		const auto = await run(['preset', 'edit', 'Everyday', '--context-window', 'auto']);
		expect(auto.out).toContain('context 200K');
		const missing = await run(['preset', 'edit', 'Opus', '--name', 'Smart']);
		expect(missing.code).not.toBe(0);
		expect(missing.err).toContain('No preset "Opus"');
	});
});

describe('nolune agent watch', () => {
	function subagentOf() {
		const { user, profile } = makeFamily();
		const chat = createConversation({ profile, presetId: makePreset().id, userId: user.id });
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'Find flights.' });
		return { env: { NOLUNE_CONVERSATION_ID: chat.id }, subagent };
	}

	it("fails with the subagent's last word when it failed", async () => {
		const { env, subagent } = subagentOf();
		setSubagentStatus(subagent.id, 'failed', 'Rate limited.');
		const watched = await run(['agent', 'watch', subagent.name], { env });
		expect(watched.code).toBe(1);
		expect(watched.out).toContain('agent-1 failed: Rate limited.');
	});

	it('stops waiting when its signal aborts', async () => {
		const { env, subagent } = subagentOf();
		const abort = new AbortController();
		const watching = run(['agent', 'watch', subagent.name], { env, signal: abort.signal });
		setTimeout(() => abort.abort(), 50);
		expect(await watching).toMatchObject({ code: 1, out: '' });
	});
});
