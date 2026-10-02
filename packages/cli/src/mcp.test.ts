import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
	closeMcpConnections,
	createProfile,
	finishMcpSignIn,
	initConfig,
	readConfig
} from '@nolune/core';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { makeUser } from '../../core/src/test/fixtures.ts';
import { startOAuthServer } from '../../core/src/test/oauth-server.ts';
import { runCli } from './run.ts';
import { testIo } from './test/io.ts';

/** The fake MCP server core's tests use, run by this computer's node. */
const FAKE = fileURLToPath(new URL('../../core/src/test/mcp-server.ts', import.meta.url));

beforeEach(() => {
	initConfig();
});

afterEach(async () => {
	await closeMcpConnections();
});

async function run(argv: string[], opts: Parameters<typeof testIo>[0] = {}) {
	const { io, out, err } = testIo(opts);
	const code = await runCli(argv, io);
	return { code, out: out(), err: err() };
}

describe('nolune mcp', () => {
	it('connects a server this computer runs, checking it first', async () => {
		const added = await run([
			'mcp',
			'add',
			'fake',
			'--env',
			'TOKEN=abc',
			'--',
			process.execPath,
			FAKE
		]);
		expect(added).toEqual({
			code: 0,
			out: 'Added fake: 8 tools (echo, picture, fail, structured, env, tag, pid, wait).\n',
			err: ''
		});
		expect(readConfig().mcpServers?.fake).toEqual({
			type: 'stdio',
			command: process.execPath,
			args: [FAKE],
			env: { TOKEN: 'abc' },
			// What the server says about itself, until someone gives a description.
			description: 'Tools for nolune tests.'
		});
		const listed = await run(['mcp', 'list']);
		expect(listed.out).toBe(
			`fake\tstdio\t${process.execPath} ${FAKE}\tenv: TOKEN\tevery profile\n  Tools for nolune tests.\n`
		);
		expect(listed.out).not.toContain('abc');
	});

	it('saves a server it cannot reach, saying why, and one at an address', async () => {
		const missing = await run(['mcp', 'add', 'gone', '--', 'no-such-mcp-server-xyz']);
		expect(missing.code).toBe(0);
		expect(missing.out).toMatch(/^Added gone, but couldn't connect to it: couldn't start gone/);
		const remote = await run([
			'mcp',
			'add',
			'remote',
			'http://127.0.0.1:1/mcp',
			'-H',
			'Authorization: Bearer t0k',
			'-t',
			'sse',
			'-d',
			'Family calendar'
		]);
		expect(remote.out).toMatch(/^Added remote, but couldn't connect to it/);
		expect(readConfig().mcpServers?.remote).toEqual({
			type: 'sse',
			url: 'http://127.0.0.1:1/mcp',
			headers: { Authorization: 'Bearer t0k' },
			description: 'Family calendar'
		});
	});

	it('takes JSON as MCP clients write it, a whole mcpServers block too', async () => {
		const json = JSON.stringify({
			mcpServers: { anything: { command: process.execPath, args: [FAKE] } }
		});
		expect((await run(['mcp', 'add-json', 'fake', json])).out).toMatch(/^Added fake: 8 tools/);
		expect(readConfig().mcpServers?.fake).toMatchObject({ type: 'stdio', args: [FAKE] });
		expect((await run(['mcp', 'add-json', 'x', '{"url": 5}'])).err).toBe(
			'nolune: Its url is an address starting with http:// or https://.\n'
		);
	});

	it('says how to give a command whose flags look like nolune’s, and refuses bad names', async () => {
		expect((await run(['mcp', 'add', 'files', 'npx', '-y', 'server'])).err).toMatch(
			/put the server's command after --/
		);
		expect((await run(['mcp', 'add', 'My Files', '--', 'npx'])).err).toMatch(
			/lowercase letters and digits/
		);
		expect((await run(['mcp', 'add', 'x', '--profile', 'nope', '--', 'npx'])).err).toMatch(
			/no profile with slug "nope"/
		);
	});

	it('lists tools, shows one, and calls them', async () => {
		await run(['mcp', 'add', 'fake', '--', process.execPath, FAKE]);
		const tools = await run(['mcp', 'tools', 'fake']);
		expect(tools.out).toContain(
			'fake: Fake Server, 8 tools\nTools for nolune tests. Echo says things back.\n'
		);
		expect(tools.out).toContain('\necho(text, shout?) [reads only]\n  Says the text back.\n');
		const echo = await run(['mcp', 'tools', 'fake', 'echo']);
		expect(echo.out).toContain(
			'fake echo [reads only]\nSays the text back.\nA second line of description.\n'
		);
		expect(echo.out).toContain('"required": [\n    "text"\n  ]');
		expect((await run(['mcp', 'tools', 'fake', 'nope'])).err).toMatch(
			/fake has no tool called nope/
		);

		expect(await run(['mcp', 'call', 'fake', 'echo', '{"text": "hi", "shout": true}'])).toEqual({
			code: 0,
			out: 'HI\n',
			err: ''
		});
		expect(
			await run(['mcp', 'call', 'fake', 'echo', '-'], { stdin: '{"text": "from stdin"}' })
		).toMatchObject({ code: 0, out: 'from stdin\n' });
		expect(await run(['mcp', 'call', 'fake', 'fail'])).toEqual({
			code: 1,
			out: 'it broke\n',
			err: 'nolune: fake fail reported an error.\n'
		});
		expect((await run(['mcp', 'call', 'fake', 'structured'])).out).toBe('{\n  "answer": 42\n}\n');
		expect((await run(['mcp', 'call', 'fake', 'echo', '[1]'])).err).toMatch(/one JSON object/);
	});

	it('shows the pictures a tool returns to the agent', async () => {
		await run(['mcp', 'add', 'fake', '--', process.execPath, FAKE]);
		const view = mkdtempSync(join(tmpdir(), 'nolune-view-'));
		writeFileSync(join(view, 'limits.json'), JSON.stringify({ count: 5 }));
		const { code, out } = await run(['mcp', 'call', 'fake', 'picture'], {
			env: { NOLUNE_VIEW_DIR: view },
			cwd: tmpdir()
		});
		expect(code).toBe(0);
		expect(out).toMatch(
			/^Here it is\.\nAttached \/.*\/nolune-mcp-[^/]+\/image-1\.png \(1×1 PNG\)\.\n$/
		);
		expect(readFileSync(join(view, 'manifest.jsonl'), 'utf8')).toContain('image-1.png');
		// At a terminal, the file.
		expect((await run(['mcp', 'call', 'fake', 'picture'])).out).toMatch(
			/\nImage: \/.*image-1\.png\n$/
		);
	});

	it("keeps to the servers the agent's profile has", async () => {
		const anna = makeUser('Anna');
		createProfile('Family', anna.id);
		createProfile('Anna', anna.id);
		await run(['mcp', 'add', 'shared', '--', process.execPath, FAKE]);
		await run(['mcp', 'add', 'annas', '--profile', 'anna', '--', process.execPath, FAKE]);
		const family = { env: { NOLUNE_PROFILE: 'family' } };
		expect((await run(['mcp', 'list'], family)).out).not.toContain('annas');
		expect((await run(['mcp', 'list'], { env: { NOLUNE_PROFILE: 'anna' } })).out).toContain(
			'annas\tstdio'
		);
		expect(await run(['mcp', 'call', 'annas', 'echo', '{"text": "x"}'], family)).toEqual({
			code: 1,
			out: '',
			err: "nolune: annas isn't connected in this profile. An admin can add it on the Connected services page.\n"
		});
		const all = await run(['mcp', 'tools'], family);
		expect(all.out).toMatch(/^shared: Fake Server, 8 tools\n/);
		expect(all.out).not.toContain('annas');
	});

	it('removes servers', async () => {
		await run(['mcp', 'add', 'gone', '--', 'no-such-mcp-server-xyz']);
		expect(await run(['mcp', 'rm', 'gone'])).toEqual({ code: 0, out: 'Removed gone.\n', err: '' });
		expect((await run(['mcp', 'list'])).out).toMatch(/^No MCP servers\. An admin connects them/);
		expect((await run(['mcp', 'call', 'gone', 'x'])).err).toMatch(
			/there's no MCP server called gone/
		);
	});

	it('signs in to a server that wants it, and out', async () => {
		const oauth = await startOAuthServer();
		try {
			const added = await run(['mcp', 'add', 'notes', oauth.url]);
			expect(added.out).toBe(
				'Added notes. It needs someone to sign in: `nolune mcp login notes`, or Sign in on the Connected services page.\n'
			);
			expect((await run(['mcp', 'list'])).out).toBe(
				`notes\thttp\t${oauth.url}\tno headers\tevery profile\tneeds sign-in (nolune mcp login notes)\n`
			);

			const login = await run(['mcp', 'login', 'notes']);
			const page = new URL(/^https?:\/\/\S+$/m.exec(login.out)![0]);
			expect(page.searchParams.get('redirect_uri')).toBe(
				'http://localhost:5780/mcp/oauth/callback'
			);
			expect(login.out).toContain('It comes back to nolune at http://localhost:5780');
			// The browser comes back to the gateway's page, which finishes it.
			const { code, state } = oauth.approve(page);
			await finishMcpSignIn(state, code);
			expect((await run(['mcp', 'list'])).out).toContain('\tsigned in\n');
			expect((await run(['mcp', 'call', 'notes', 'echo', '{"text": "hi"}'])).out).toBe('hi\n');

			expect((await run(['mcp', 'logout', 'notes'])).out).toBe(
				'Signed out of notes: nolune forgot its tokens.\n'
			);
			expect((await run(['mcp', 'list'])).out).toContain('needs sign-in');
		} finally {
			await closeMcpConnections();
			await oauth.close();
		}
	});

	it('keeps the app a service had someone register for its sign-in', async () => {
		const added = await run([
			'mcp',
			'add',
			'notes',
			'http://127.0.0.1:1/mcp',
			'--client-id',
			'family-app',
			'--client-secret',
			'shh',
			'--scope',
			'notes.read'
		]);
		expect(added.code).toBe(0);
		expect(readConfig().mcpServers?.notes).toEqual({
			type: 'http',
			url: 'http://127.0.0.1:1/mcp',
			oauth: { clientId: 'family-app', clientSecret: 'shh', scope: 'notes.read' }
		});
		expect((await run(['mcp', 'list'])).out).not.toContain('shh');
		const wrong = await run(['mcp', 'add', 'home', '--client-id', 'x', '--', 'home-mcp']);
		expect(wrong.code).toBe(1);
		expect(wrong.err).toContain('--client-id, --client-secret and --scope are for signing in');
		await run(['mcp', 'add', 'home', '--', 'no-such-mcp-server-xyz']);
		const login = await run(['mcp', 'login', 'home']);
		expect(login.code).toBe(1);
		expect(login.err).toContain('home runs on this computer');
	});
});
