import { fileURLToPath } from 'node:url';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';

/*
 * A small MCP server for tests, over stdio: `node mcp-server.ts`. Its tools cover what
 * `nolune mcp` has to handle: text, a picture, an error, structured output, lists and choices in
 * its arguments, its environment and a slow call. FAKE_MCP_FAIL makes it stop before it answers, saying why on stderr.
 */

if (process.env.FAKE_MCP_FAIL) {
	console.error(`fake server: ${process.env.FAKE_MCP_FAIL}`);
	process.exit(1);
}

/** A 1×1 PNG. */
export const PIXEL =
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

export function fakeServer(): McpServer {
	const server = new McpServer(
		{ name: 'fake', title: 'Fake Server', version: '1.0.0' },
		{ instructions: 'Tools for nolune tests. Echo says things back.' }
	);
	server.registerTool(
		'echo',
		{
			description: 'Says the text back.\nA second line of description.',
			inputSchema: { text: z.string(), shout: z.boolean().optional() },
			annotations: { readOnlyHint: true }
		},
		({ text, shout }) => ({ content: [{ type: 'text', text: shout ? text.toUpperCase() : text }] })
	);
	server.registerTool('picture', { description: 'A picture.' }, () => ({
		content: [
			{ type: 'text', text: 'Here it is.' },
			{ type: 'image', data: PIXEL, mimeType: 'image/png' }
		]
	}));
	server.registerTool('fail', { description: 'Always fails.' }, () => ({
		content: [{ type: 'text', text: 'it broke' }],
		isError: true
	}));
	server.registerTool(
		'structured',
		{ description: 'Only structured output.', annotations: { destructiveHint: true } },
		() => ({ content: [], structuredContent: { answer: 42 } })
	);
	server.registerTool(
		'env',
		{ description: 'An environment variable.', inputSchema: { name: z.string() } },
		({ name }) => ({ content: [{ type: 'text', text: process.env[name] ?? '(unset)' }] })
	);
	server.registerTool(
		'tag',
		{
			description: 'Tags things.',
			inputSchema: {
				tags: z.array(z.string()),
				color: z.enum(['red', 'blue']),
				where: z.object({ x: z.number() }).optional()
			}
		},
		({ tags, color }) => ({ content: [{ type: 'text', text: `${color}: ${tags.join(', ')}` }] })
	);
	server.registerTool('pid', { description: 'Its process id.' }, () => ({
		content: [{ type: 'text', text: String(process.pid) }]
	}));
	server.registerTool(
		'wait',
		{ description: 'Waits.', inputSchema: { ms: z.number() } },
		async ({ ms }) => {
			await new Promise((resolve) => setTimeout(resolve, ms));
			return { content: [{ type: 'text', text: 'done' }] };
		}
	);
	return server;
}

// Run as a program, rather than imported by a test.
if (process.argv[1] === fileURLToPath(import.meta.url)) {
	await fakeServer().connect(new StdioServerTransport());
}
