import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
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

		const added = await run(['memory', 'add', 'home', 'Anna is 7'], {
			env: { NOLUNE_PROFILE: 'family' }
		});

		expect(added).toEqual({ code: 0, out: 'Started home.md.\n', err: '' });
		expect(readMemoryNote('family', 'home').text).toContain('Anna is 7');
		expect(() => readMemoryNote('work', 'home')).toThrow();
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

	it('searches memory, with where each fact is', async () => {
		createProfile('Family', makeUser('Anna').id);
		const env = { NOLUNE_PROFILE: 'family' };
		await run(['memory', 'write', 'home'], {
			env,
			stdin: '# Home\n\n## Internet\n\n- Wifi password: mango42\n- Router in the hall\n'
		});

		expect(await run(['memory', 'search', 'wifi', 'passwords'], { env })).toEqual({
			code: 0,
			out: 'home.md:5  Wifi password: mango42  (Internet)\n',
			err: ''
		});
		expect((await run(['memory', 'search', 'dentist'], { env })).out).toContain(
			'Nothing in Family\'s memory matches "dentist"'
		);
	});

	it('lists whose notes are whose, and merges two about one person', async () => {
		createProfile('Family', makeUser('Anna').id);
		const env = { NOLUNE_PROFILE: 'family' };
		await run(['memory', 'add', 'people/grandma', 'Loves roses'], { env });
		await run(['memory', 'add', 'people/olga', 'Lives in Tver'], { env });
		expect(await run(['memory', 'add', 'family', 'Olga is 70'], { env })).toMatchObject({
			code: 1,
			err: expect.stringContaining('"family" isn\'t one of memory\'s categories')
		});

		const list = (await run(['memory'], { env })).out;
		expect(list).toContain(
			"Anna's note people/anna.md is started when there is something to write."
		);
		expect(list).toMatch(/people\/olga\.md +1 fact/);

		expect(
			await run(['memory', 'add', 'people/olga', "Who: Tester's grandmother\n- Lives in Tver"], {
				env
			})
		).toMatchObject({ out: 'Saved to people/olga.md: 1 fact, and 1 it already had.\n' });

		expect(await run(['memory', 'merge', 'people/grandma', 'people/olga'], { env })).toEqual({
			code: 0,
			out: "Merged people/grandma.md into people/olga.md: 1 part it didn't have.\n",
			err: ''
		});
		expect(readMemoryNote('family', 'people/olga').text).toBe(
			"# Olga\n\n- Lives in Tver\n- Who: Tester's grandmother\n- Loves roses\n- Also called: Grandma\n"
		);
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

describe('nolune provider', () => {
	it('checks a custom provider for its models, and presets and memory search can use it', async () => {
		// A server that wants a key, like a vLLM started with --api-key.
		const server = createServer((req, res) => {
			res.setHeader('content-type', 'application/json');
			if (req.headers.authorization !== 'Bearer sk-local') {
				res.writeHead(401).end(JSON.stringify({ error: { message: 'Invalid API key' } }));
			} else if (req.url === '/v1/models') {
				res.end(JSON.stringify({ object: 'list', data: [{ id: 'qwen3:8b' }, { id: 'nomic' }] }));
			} else {
				res.end(JSON.stringify({ data: [{ index: 0, embedding: [0.6, 0.8] }] }));
			}
		});
		await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
		const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
		try {
			const refused = await run(['provider', 'add', 'GPU box', url], { stdin: '' });
			expect(refused.code).not.toBe(0);
			expect(refused.err).toContain('The server wants a key.');

			const args = ['provider', 'add', 'GPU box', `${url}/`, '--api', 'anthropic'];
			expect(await run([...args, '--key', 'sk-local'])).toEqual({
				code: 0,
				out: `Added GPU box at ${url}, through its Anthropic API. It serves qwen3:8b, nomic.\n`,
				err: ''
			});
			// The same server through its OpenAI API, for memory search.
			await run(['provider', 'add', 'Embeddings', url, '--key', 'sk-local']);
			expect((await run(['provider', 'list'])).out).toBe(
				`GPU box\tgpu-box\tanthropic\t${url}\tkey\nEmbeddings\tembeddings\topenai\t${url}\tkey\n`
			);
			expect((await run(['config'])).out).toContain(
				`\ncustom        GPU box ${url} (anthropic, with a key), Embeddings ${url} (openai, with a key)\n`
			);
			const other = await run(['provider', 'add', 'gpu box', url, '--api', 'openai']);
			expect(other.err).toContain("GPU box speaks Anthropic's API.");

			// Picked by its name; the preset is named by it.
			const added = await run(['preset', 'add', 'qwen3:8b', '--provider', 'GPU box']);
			expect(added.out).toBe('Added "qwen3:8b (GPU box)" (context ?).\n');
			expect((await run(['preset', 'list'])).out).toContain('GPU box/qwen3:8b');
			const unknown = await run(['preset', 'add', 'qwen3:8b', '--provider', 'ollama']);
			expect(unknown.err).toContain('no provider "ollama"');

			const meaning = await run(['config', 'set', 'embeddings', 'custom-openai/embeddings/nomic']);
			expect(meaning.out).toBe('Memory search by meaning: Embeddings/nomic.\n');
			const address = await run(['config', 'set', 'embeddings', url]);
			expect(address.err).toContain('nolune provider add <name> <url>');

			expect((await run(['provider', 'rm', 'gpu-box'])).out).toBe(
				'Removed GPU box. Chats on "qwen3:8b (GPU box)" stop working until they\'re moved to another model.\n'
			);
			await run(['provider', 'rm', 'Embeddings']);
			expect((await run(['config'])).out).toContain(
				'embeddings    custom-openai/embeddings/nomic, but there is no custom provider "embeddings"'
			);
		} finally {
			server.close();
		}
	});

	it("saves one that doesn't answer yet, with a warning", async () => {
		const saved = await run(['provider', 'add', 'Later', 'http://127.0.0.1:1']);
		expect(saved.code).toBe(0);
		expect(saved.out).toMatch(
			/^Saved Later without checking it\. Couldn't reach http:\/\/127\.0\.0\.1:1 \(.+\)\.\n$/
		);
		const reserved = await run(['provider', 'add', 'OpenAI', 'http://127.0.0.1:1']);
		expect(reserved.err).toContain("There's already a provider called OpenAI.");
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

describe('command mode', () => {
	it("is set at the terminal, never from the agent's own commands", async () => {
		makePreset('Haiku', 'claude-haiku-5');
		expect((await run(['config'])).out).toContain(
			"\ncommands      auto mode, checked by each chat's own model\n"
		);

		expect(await run(['config', 'set', 'safety-model', 'Haiku'])).toEqual({
			code: 0,
			out: 'Commands: auto mode, checked by Haiku.\n',
			err: ''
		});
		expect(await run(['config', 'set', 'command-mode', 'unrestricted'])).toMatchObject({
			code: 0,
			out: 'Commands: unrestricted: commands run without a check.\n'
		});
		expect((await run(['config', 'set', 'command-mode', 'off'])).err).toContain(
			'command-mode is auto or unrestricted'
		);
		expect((await run(['config', 'set', 'safety-model', 'Opus'])).err).toContain(
			'No preset "Opus"'
		);

		const fromAgent = await run(['config', 'set', 'command-mode', 'auto'], {
			env: { NOLUNE_CONVERSATION_ID: 'chat-1' }
		});
		expect(fromAgent.code).toBe(1);
		expect(fromAgent.err).toContain("the agent can't change how its own commands are checked");
		expect((await run(['config'])).out).toContain('commands      unrestricted');

		await run(['config', 'set', 'command-mode', 'auto']);
		await run(['config', 'set', 'safety-model', 'chat']);
		expect((await run(['config'])).out).toContain(
			"commands      auto mode, checked by each chat's own model"
		);
	});
});
