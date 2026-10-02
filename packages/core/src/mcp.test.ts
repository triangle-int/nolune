import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer, type Server as HttpServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { initConfig, readConfig, updateConfig } from './config.ts';
import {
	McpServerError,
	callMcpTool,
	checkMcpServer,
	closeMcpConnections,
	describeFromServer,
	findMcpServer,
	holdMcpConnections,
	joinCommandLine,
	listMcpServers,
	listMcpTools,
	mcpResultText,
	mcpToolName,
	mcpToolServer,
	parseMcpServer,
	refreshMcpTools,
	removeMcpServer,
	saveMcpServer,
	splitCommandLine,
	type McpServerConfig
} from './mcp.ts';
import { fakeServer } from './test/mcp-server.ts';

const FAKE = fileURLToPath(new URL('./test/mcp-server.ts', import.meta.url));

/** The fake server, as this computer would run it. */
function fake(extra: Partial<McpServerConfig> = {}): McpServerConfig {
	return { type: 'stdio', command: process.execPath, args: [FAKE], ...extra } as McpServerConfig;
}

function text(result: Awaited<ReturnType<typeof callMcpTool>>): string {
	return result.content.map((c) => (c.type === 'text' ? c.text : `[${c.type}]`)).join('\n');
}

beforeEach(() => {
	initConfig();
});

afterEach(async () => {
	await closeMcpConnections();
});

describe('parseMcpServer', () => {
	it('reads a command as MCP clients write it, leaving out what nolune does not use', () => {
		expect(
			parseMcpServer({
				command: ' npx ',
				args: ['-y', '@modelcontextprotocol/server-github'],
				env: { GITHUB_TOKEN: 'ghp_x' },
				disabled: false,
				alwaysAllow: []
			})
		).toEqual({
			type: 'stdio',
			command: 'npx',
			args: ['-y', '@modelcontextprotocol/server-github'],
			env: { GITHUB_TOKEN: 'ghp_x' }
		});
	});

	it('reads an address: http by default, sse when it says so', () => {
		expect(parseMcpServer({ url: 'https://example.com/mcp' })).toEqual({
			type: 'http',
			url: 'https://example.com/mcp'
		});
		expect(
			parseMcpServer({
				type: 'sse',
				url: 'http://localhost:8000/sse',
				headers: { Authorization: 'Bearer x' },
				description: '  The   family  calendar ',
				profiles: ['family', 'family']
			})
		).toEqual({
			type: 'sse',
			url: 'http://localhost:8000/sse',
			headers: { Authorization: 'Bearer x' },
			description: 'The family calendar',
			profiles: ['family']
		});
		expect(parseMcpServer({ type: 'streamable-http', url: 'https://x.dev' }).type).toBe('http');
	});

	it('says what is wrong', () => {
		expect(() => parseMcpServer('npx')).toThrow(McpServerError);
		expect(() => parseMcpServer({})).toThrow(/needs a command to run or a url/);
		expect(() => parseMcpServer({ type: 'http', url: 'ftp://x' })).toThrow(/http:\/\/ or https/);
		expect(() => parseMcpServer({ command: 'x', env: { 'BAD NAME': 'v' } })).toThrow(
			/"BAD NAME" can't be/
		);
		expect(() => parseMcpServer({ command: 'x', args: 'serve' })).toThrow(/list of texts/);
		expect(() => parseMcpServer({ type: 'websocket', command: 'x' })).toThrow(/stdio, http or sse/);
	});
});

describe('command lines', () => {
	it('splits as a shell does and joins back', () => {
		const words = splitCommandLine(
			`npx -y "@scope/pkg" --dir '/Users/anna/My Files' a\\ b "q\\"t"`
		);
		expect(words).toEqual([
			'npx',
			'-y',
			'@scope/pkg',
			'--dir',
			'/Users/anna/My Files',
			'a b',
			'q"t'
		]);
		expect(splitCommandLine(joinCommandLine(words))).toEqual(words);
		expect(joinCommandLine(['uvx', 'mcp-server-time', "it's"])).toBe(
			`uvx mcp-server-time 'it'\\''s'`
		);
		expect(() => splitCommandLine(`npx "open`)).toThrow(/isn't closed/);
	});
});

describe('saving servers', () => {
	it('keeps them in config.json and shows them without their keys', () => {
		saveMcpServer('github', {
			type: 'http',
			url: 'https://api.example.com/mcp',
			headers: { Authorization: 'Bearer secret' },
			description: 'Issues and pull requests'
		});
		saveMcpServer('home', fake({ env: { HASS_TOKEN: 't' }, profiles: ['family'] }));
		expect(readConfig().mcpServers?.github).toMatchObject({
			headers: { Authorization: 'Bearer secret' }
		});
		const listed = listMcpServers();
		expect(listed.map((s) => s.name)).toEqual(['github', 'home']);
		expect(listed[0]).toEqual({
			name: 'github',
			type: 'http',
			target: 'https://api.example.com/mcp',
			cwd: null,
			secrets: ['Authorization'],
			description: 'Issues and pull requests',
			profiles: null,
			problem: null
		});
		expect(JSON.stringify(listed)).not.toContain('Bearer');
		expect(listed[1]).toMatchObject({ secrets: ['HASS_TOKEN'], profiles: ['family'] });
		expect(listed[1].target).toBe(joinCommandLine([process.execPath, FAKE]));
	});

	it('gives each profile the servers it has', () => {
		saveMcpServer('everyone', fake({ description: 'For all' }));
		saveMcpServer('annas', fake({ profiles: ['anna'] }));
		expect(listMcpServers('family').map((s) => s.name)).toEqual(['everyone']);
		expect(listMcpServers('anna').map((s) => s.name)).toEqual(['everyone', 'annas']);
		expect(() => findMcpServer('annas', 'family')).toThrow(/isn't connected in this profile/);
		expect(findMcpServer('annas', 'anna').type).toBe('stdio');
	});

	it('keeps saved keys only when asked, and only for the same kind of server', () => {
		saveMcpServer('home', fake({ env: { HASS_TOKEN: 't' } }));
		saveMcpServer('home', fake({ description: 'Lights' }), { keepSecrets: true });
		expect(readConfig().mcpServers?.home).toMatchObject({ env: { HASS_TOKEN: 't' } });
		saveMcpServer('home', fake(), {});
		expect(readConfig().mcpServers?.home).not.toHaveProperty('env');
		saveMcpServer('home', fake({ env: { HASS_TOKEN: 't' } }));
		saveMcpServer('home', { type: 'http', url: 'https://x.dev' }, { keepSecrets: true });
		expect(readConfig().mcpServers?.home).toEqual({ type: 'http', url: 'https://x.dev' });
	});

	it('refuses bad names and removes them', () => {
		expect(() => saveMcpServer('My Server', fake())).toThrow(/lowercase letters and digits/);
		expect(() => removeMcpServer('nope')).toThrow(/no MCP server called nope/);
		saveMcpServer('a', fake());
		removeMcpServer('a');
		expect(readConfig()).not.toHaveProperty('mcpServers');
		expect(() => findMcpServer('a')).toThrow(/admin connects them on the Connected services page/);
	});

	it('says when its settings in config.json are broken', () => {
		updateConfig((c) => {
			c.mcpServers = { broken: { type: 'stdio' } as McpServerConfig };
		});
		expect(listMcpServers()[0].problem).toMatch(/needs a command/);
		expect(() => findMcpServer('broken')).toThrow(/settings in config.json are broken/);
	});
});

describe('a server this computer runs', () => {
	it('lists its tools and what it says about itself', async () => {
		saveMcpServer('fake', fake());
		const found = await listMcpTools('fake');
		expect(found.title).toBe('Fake Server');
		expect(found.instructions).toBe('Tools for nolune tests. Echo says things back.');
		expect(found.tools.map((t) => t.name)).toContain('echo');
		expect(describeFromServer(found)).toBe('Tools for nolune tests.');
	});

	it('calls tools and hands back what they returned', async () => {
		saveMcpServer('fake', fake());
		expect(text(await callMcpTool('fake', 'echo', { text: 'hi', shout: true }))).toBe('HI');
		expect(text(await callMcpTool('fake', 'picture', {}))).toBe('Here it is.\n[image]');
		const failed = await callMcpTool('fake', 'fail', {});
		expect(failed.isError).toBe(true);
		expect((await callMcpTool('fake', 'structured', {})).structuredContent).toEqual({ answer: 42 });
	});

	it("gets the agent's commands' environment without nolune's secrets, and its own", async () => {
		process.env.ANTHROPIC_API_KEY = 'sk-ant-secret';
		updateConfig((c) => {
			c.commandEnv = { FIRECRAWL_API_KEY: 'fc-1' };
		});
		try {
			saveMcpServer('fake', fake({ env: { OWN: 'mine' } }));
			const env = async (name: string) => text(await callMcpTool('fake', 'env', { name }));
			expect(await env('OWN')).toBe('mine');
			expect(await env('FIRECRAWL_API_KEY')).toBe('fc-1');
			expect(await env('ANTHROPIC_API_KEY')).toBe('(unset)');
		} finally {
			delete process.env.ANTHROPIC_API_KEY;
		}
	});

	it('says why it could not start', async () => {
		saveMcpServer('broken', fake({ env: { FAKE_MCP_FAIL: 'no token given' } }));
		await expect(listMcpTools('broken')).rejects.toThrow(
			/broken stopped before it answered\. It said: fake server: no token given/
		);
		saveMcpServer('missing', { type: 'stdio', command: 'no-such-mcp-server-xyz' });
		await expect(listMcpTools('missing')).rejects.toThrow(
			/there's no no-such-mcp-server-xyz on this computer/
		);
	});

	it('stops a call when the command is stopped', async () => {
		saveMcpServer('fake', fake());
		const stop = new AbortController();
		setTimeout(() => stop.abort(new Error('stopped')), 300);
		const started = Date.now();
		await expect(
			callMcpTool('fake', 'wait', { ms: 10_000 }, { signal: stop.signal })
		).rejects.toThrow();
		expect(Date.now() - started).toBeLessThan(5000);
	});

	it('connects for each command outside the gateway', async () => {
		saveMcpServer('fake', fake());
		const first = text(await callMcpTool('fake', 'pid', {}));
		const second = text(await callMcpTool('fake', 'pid', {}));
		expect(first).not.toBe(second);
	});

	it('checks settings before they are saved, on a connection of their own', async () => {
		const found = await checkMcpServer('new', fake());
		expect(found.tools.length).toBeGreaterThan(3);
		await expect(
			checkMcpServer('new', { type: 'stdio', command: 'no-such-mcp-server-xyz' })
		).rejects.toThrow(McpServerError);
	});
});

describe('a server at an address', () => {
	let http: HttpServer;
	let base: string;

	beforeAll(async () => {
		http = createServer(async (req, res) => {
			if (!req.url?.startsWith('/mcp')) {
				res.writeHead(404).end('Not found');
				return;
			}
			if (req.headers.authorization !== 'Bearer letmein') {
				res.writeHead(401).end('Unauthorized');
				return;
			}
			// Stateless: a server of its own for every request.
			const server = fakeServer();
			const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined });
			res.on('close', () => {
				void transport.close();
				void server.close();
			});
			await server.connect(transport);
			await transport.handleRequest(req, res);
		});
		await new Promise<void>((resolve) => http.listen(0, '127.0.0.1', resolve));
		base = `http://127.0.0.1:${(http.address() as AddressInfo).port}`;
	});

	afterAll(async () => {
		http.closeAllConnections();
		await new Promise((resolve) => http.close(resolve));
	});

	it('sends its headers', async () => {
		saveMcpServer('remote', {
			type: 'http',
			url: `${base}/mcp`,
			headers: { Authorization: 'Bearer letmein' }
		});
		expect(text(await callMcpTool('remote', 'echo', { text: 'over http' }))).toBe('over http');
	});

	it('says when it wants a key, or is not there', async () => {
		await expect(checkMcpServer('remote', { type: 'http', url: `${base}/mcp` })).rejects.toThrow(
			/turned nolune away \(401\): it wants a key or token/
		);
		await expect(
			checkMcpServer('remote', {
				type: 'http',
				url: `${base}/mcp`,
				headers: { Authorization: 'Bearer nope' }
			})
		).rejects.toThrow(/its key or token wasn't accepted/);
		await expect(
			checkMcpServer('remote', {
				type: 'http',
				url: `${base}/elsewhere`,
				headers: { Authorization: 'Bearer letmein' }
			})
		).rejects.toThrow(/no MCP server at .*\/elsewhere \(404\)/);
		// A port nothing listens on any more.
		const closed = createServer();
		await new Promise<void>((resolve) => closed.listen(0, '127.0.0.1', resolve));
		const { port } = closed.address() as AddressInfo;
		await new Promise((resolve) => closed.close(resolve));
		await expect(
			checkMcpServer('remote', { type: 'http', url: `http://127.0.0.1:${port}/mcp` })
		).rejects.toThrow(/couldn't reach remote at http:\/\/127\.0\.0\.1:\d+\/mcp: .*ECONNREFUSED/);
	});
});

describe("a chat's tools", () => {
	it("names a server's tools mcp__<server>__<tool>, within what providers take", () => {
		expect(mcpToolName('github', 'search_issues')).toBe('mcp__github__search_issues');
		const dotted = mcpToolName('home', 'lights.turn_on');
		expect(dotted).toMatch(/^mcp__home__lights_turn_on_[0-9a-f]{6}$/);
		expect(dotted).not.toBe(mcpToolName('home', 'lights_turn.on'));
		const long = mcpToolName('a-rather-long-server-name-here', 'and_an_even_longer_tool_name_too');
		expect(long.length).toBeLessThanOrEqual(51);
		expect(long).toMatch(/^mcp__a-rather-long-server-name-here__and_an_[0-9a-f]{6}$/);
		expect(mcpToolServer('mcp__home-assistant__turn_on')).toBe('home-assistant');
		expect(mcpToolServer('mcp__my_server__get__thing')).toBe('my_server');
		expect(mcpToolServer('run_command')).toBeNull();
	});

	it('remembers what servers said their tools are, and forgets a removed one', async () => {
		saveMcpServer('fake', fake());
		await refreshMcpTools();
		const known = JSON.parse(
			readFileSync(join(process.env.NOLUNE_HOME!, 'mcp-tools.json'), 'utf8')
		);
		expect(known.fake.tools.map((t: { name: string }) => t.name)).toContain('echo');
		expect(known.fake.instructions).toBe('Tools for nolune tests. Echo says things back.');
		removeMcpServer('fake');
		expect(readFileSync(join(process.env.NOLUNE_HOME!, 'mcp-tools.json'), 'utf8')).toBe('{}\n');
	});

	it('turns what a tool returned into text, showing its pictures to the agent', async () => {
		saveMcpServer('fake', fake());
		const view = mkdtempSync(join(tmpdir(), 'nolune-view-'));
		writeFileSync(join(view, 'limits.json'), JSON.stringify({ count: 5 }));
		const text = await mcpResultText(await callMcpTool('fake', 'picture', {}), view);
		expect(text).toMatch(/^Here it is\.\nAttached \/.*\/image-1\.png \(1×1 PNG\)\.$/);
		expect(await mcpResultText(await callMcpTool('fake', 'structured', {}))).toBe(
			'{\n  "answer": 42\n}'
		);
	});
});

// Last: the gateway's way can't be turned off again in this process.
describe('in the gateway', () => {
	it('keeps a connection between commands, and opens another when its settings change', async () => {
		holdMcpConnections();
		saveMcpServer('fake', fake());
		const first = text(await callMcpTool('fake', 'pid', {}));
		expect(text(await callMcpTool('fake', 'pid', {}))).toBe(first);
		// Concurrent calls share it too.
		const both = await Promise.all([callMcpTool('fake', 'pid', {}), listMcpTools('fake')]);
		expect(text(both[0])).toBe(first);
		// What's only for people and the agent doesn't need a new one.
		saveMcpServer('fake', fake({ description: 'Now with a description' }));
		expect(text(await callMcpTool('fake', 'pid', {}))).toBe(first);
		saveMcpServer('fake', fake({ env: { NEW: '1' } }));
		const second = text(await callMcpTool('fake', 'pid', {}));
		expect(second).not.toBe(first);
		expect(() => process.kill(Number(first), 0)).toThrow();
	});
});
