import { fileURLToPath } from 'node:url';
import type Anthropic from '@anthropic-ai/sdk';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createMessage, streamTurn } from './anthropic.ts';
import { saveCommandMode } from './command-safety.ts';
import {
	appendRow,
	commitQueuedRows,
	committedRows,
	createConversation,
	getConversation,
	insertQueued,
	toDisplay
} from './conversations.ts';
import {
	closeMcpConnections,
	refreshMcpTools,
	removeMcpServer,
	saveMcpServer,
	type McpServerConfig
} from './mcp.ts';
import { TOOLS } from './run-command.ts';
import {
	ReloadError,
	chatToolChanges,
	kick,
	onLoopEnd,
	reloadTools,
	withCurrentContext
} from './runner.ts';
import { writeSoul } from './soul.ts';
import { makeFamily, makePreset, runCommandsUnchecked } from './test/fixtures.ts';

/*
 * MCP servers' tools in chats: a new chat gets its profile's servers' tools next to run_command,
 * and its prompt says what they're for; the runner calls them, auto mode checks them, and the
 * chat shows them. Against the fake server core's other tests use, with the model's replies lined
 * up.
 */

vi.mock('./anthropic.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./anthropic.ts')>()),
	streamTurn: vi.fn(),
	// Auto mode's checks, on the chat's own model.
	createMessage: vi.fn()
}));

const FAKE = fileURLToPath(new URL('./test/mcp-server.ts', import.meta.url));

function fake(extra: Partial<McpServerConfig> = {}): McpServerConfig {
	return { type: 'stdio', command: process.execPath, args: [FAKE], ...extra } as McpServerConfig;
}

function modelReply(content: unknown[], stopReason: Anthropic.StopReason): Anthropic.Message {
	return {
		id: 'msg',
		type: 'message',
		role: 'assistant',
		model: 'claude-sonnet-5',
		content,
		stop_reason: stopReason,
		stop_sequence: null,
		usage: { input_tokens: 10, output_tokens: 5 }
	} as unknown as Anthropic.Message;
}

const said = (text: string) => modelReply([{ type: 'text', text }], 'end_turn');

const call = (id: string, name: string, input: unknown = {}) => ({
	type: 'tool_use',
	id,
	name,
	input
});

/** The family's profile, with the fake server connected and its tools known. */
async function connected(extra: Partial<McpServerConfig> = {}) {
	const family = makeFamily();
	saveMcpServer('fake', fake({ description: 'For tests', ...extra }));
	await refreshMcpTools();
	return family;
}

let presets = 0;

/** A named chat where Anna asked for something, with the model's replies lined up. */
function chatAsking(family: ReturnType<typeof makeFamily>, ...replies: Anthropic.Message[]) {
	const chat = createConversation({
		profile: family.profile,
		presetId: makePreset(`Sonnet ${++presets}`).id,
		userId: family.user.id,
		title: 'Test'
	});
	insertQueued({
		conversationId: chat.id,
		senderId: family.user.id,
		senderName: 'Anna',
		text: 'Say hi through the fake server'
	});
	for (const reply of replies) vi.mocked(streamTurn).mockResolvedValueOnce(reply);
	return chat;
}

/** Starts the agent and resolves when its loop stops. */
function run(conversationId: string): Promise<void> {
	return new Promise((resolve) => {
		const off = onLoopEnd((id) => {
			if (id !== conversationId) return;
			off();
			resolve();
		});
		kick(conversationId);
	});
}

function results(conversationId: string) {
	return committedRows(conversationId)
		.filter((row) => row.kind === 'tool_results')
		.flatMap((row) => JSON.parse(row.content));
}

beforeEach(() => {
	runCommandsUnchecked();
});

afterEach(async () => {
	vi.resetAllMocks();
	await closeMcpConnections();
});

describe('a new chat', () => {
	it("gets its profile's servers' tools next to run_command, and its prompt says what they're for", async () => {
		const family = await connected();
		saveMcpServer('elsewhere', fake({ profiles: ['someone-else'] }));
		await refreshMcpTools();
		const chat = chatAsking(family);

		expect(chat.tools?.map((t) => t.name)).toEqual([
			'run_command',
			'mcp__fake__echo',
			'mcp__fake__picture',
			'mcp__fake__fail',
			'mcp__fake__structured',
			'mcp__fake__env',
			'mcp__fake__tag',
			'mcp__fake__pid',
			'mcp__fake__wait'
		]);
		expect(chat.tools?.[1]).toEqual({
			name: 'mcp__fake__echo',
			description: 'Says the text back.\nA second line of description.',
			input_schema: {
				type: 'object',
				properties: { text: { type: 'string' }, shout: { type: 'boolean' } },
				required: ['text']
			}
		});
		expect(chat.systemPrompt).toContain(
			'# Connected services\nSome of your tools belong to apps and services the family connected to nolune as MCP servers: their names start with mcp__<server>__.'
		);
		expect(chat.systemPrompt).toContain(
			'- fake: For tests\n  <instructions>\nTools for nolune tests. Echo says things back.\n  </instructions>'
		);
		expect(chat.systemPrompt).not.toContain('elsewhere');
	});

	it('is told of servers whose tools it lacks', async () => {
		const family = await connected();
		saveMcpServer('later', fake());
		const chat = chatAsking(family);

		expect(chat.tools?.some((t) => t.name === 'mcp__fake__echo')).toBe(true);
		expect(chat.tools?.some((t) => t.name.startsWith('mcp__later__'))).toBe(false);
		expect(chat.systemPrompt).toContain(
			"The tools of later aren't among yours in this conversation (nolune doesn't know them yet, or there are too many): `nolune mcp tools <server>` lists them"
		);
	});
});

describe('a chat already going', () => {
	/** Anna says something in the chat, which the model answers without a command. */
	function exchange(conversationId: string, family: ReturnType<typeof makeFamily>) {
		say(conversationId, family, 'Hi');
		return appendRow({
			conversationId,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'Hello!' }])
		});
	}

	function say(conversationId: string, family: ReturnType<typeof makeFamily>, text: string) {
		insertQueued({ conversationId, senderId: family.user.id, senderName: 'Anna', text });
		commitQueuedRows(conversationId);
	}

	it('keeps its tools until someone reloads them, then has the servers’ as they are now', async () => {
		const family = makeFamily();
		const chat = chatAsking(family);
		expect(chat.tools).toBe(TOOLS);
		const answered = exchange(chat.id, family);

		saveMcpServer('fake', fake({ description: 'For tests' }));
		await refreshMcpTools();
		say(chat.id, family, 'And now?');
		const stored = getConversation(chat.id)!;
		expect(withCurrentContext(stored, committedRows(chat.id))).toBe(stored);
		expect(chatToolChanges(chat.id)).toEqual({
			skills: null,
			services: { added: ['fake'], changed: [], removed: [] }
		});

		expect(reloadTools(chat.id)).toEqual({
			skills: null,
			services: { added: ['fake'], changed: [], removed: [] }
		});
		const reloaded = getConversation(chat.id)!;
		expect(reloaded.tools?.map((t) => t.name).slice(0, 3)).toEqual([
			'run_command',
			'mcp__fake__echo',
			'mcp__fake__picture'
		]);
		expect(reloaded.tools?.[0]).toEqual(TOOLS[0]);
		expect(reloaded.systemPrompt).toContain('- fake: For tests');
		// Its earlier thinking was made under the old prompt and tools.
		expect(reloaded.promptChangedAtSeq).toBe(answered.seq);
		expect(chatToolChanges(chat.id)).toBeNull();

		// Up to date: reloading again changes nothing, so the cache stays.
		expect(reloadTools(chat.id)).toBeNull();
		expect(getConversation(chat.id)).toEqual(reloaded);
	});

	it('says what a reload brings, and loses the tools of a server disconnected since', async () => {
		const family = await connected();
		const chat = chatAsking(family);
		exchange(chat.id, family);
		// Stopped in the middle of a step: the model's thinking must go back with its results.
		appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([call('m1', 'mcp__fake__echo', { text: 'x' })])
		});

		removeMcpServer('fake');
		expect(chatToolChanges(chat.id)).toEqual({
			skills: null,
			services: { added: [], changed: [], removed: ['fake'] }
		});
		expect(() => reloadTools(chat.id)).toThrow(ReloadError);
		expect(getConversation(chat.id)?.tools).toEqual(chat.tools);

		const done = appendRow({
			conversationId: chat.id,
			role: 'assistant',
			kind: 'assistant',
			content: JSON.stringify([{ type: 'text', text: 'Done.' }])
		});
		expect(reloadTools(chat.id)?.services?.removed).toEqual(['fake']);
		const reloaded = getConversation(chat.id)!;
		expect(reloaded.tools).toEqual(TOOLS);
		expect(reloaded.systemPrompt).not.toContain('Connected services');
		expect(reloaded.promptChangedAtSeq).toBe(done.seq);
	});

	it('gets them along when its prompt is built again for a new soul', async () => {
		const family = makeFamily();
		const chat = chatAsking(family);
		exchange(chat.id, family);
		saveMcpServer('fake', fake());
		await refreshMcpTools();

		writeSoul(family.profile.slug, 'You are calm.');
		say(chat.id, family, 'And now?');
		const rebuilt = withCurrentContext(getConversation(chat.id)!, committedRows(chat.id));
		expect(rebuilt.tools?.some((t) => t.name === 'mcp__fake__echo')).toBe(true);
		expect(rebuilt.systemPrompt).toContain('# Connected services');
	});

	it('sends the reloaded tools and prompt with its next request, not while it works', async () => {
		const family = makeFamily();
		const chat = chatAsking(family, said('Hi!'));
		await run(chat.id);

		saveMcpServer('fake', fake());
		await refreshMcpTools();
		reloadTools(chat.id);
		insertQueued({
			conversationId: chat.id,
			senderId: family.user.id,
			senderName: 'Anna',
			text: 'Echo something'
		});
		vi.mocked(streamTurn).mockImplementationOnce(async () => {
			expect(() => reloadTools(chat.id)).toThrow('nolune is working in this chat');
			return said('Done.');
		});
		await run(chat.id);

		const request = vi.mocked(streamTurn).mock.calls[1][0];
		expect(request.tools.map((t) => t.name)).toContain('mcp__fake__echo');
		expect(request.system).toContain('# Connected services');
		// The reply from before goes back without its thinking.
		expect(request.messages[1]).toMatchObject({ role: 'assistant', beforePromptChange: true });
	});
});

describe('a call to a server’s tool', () => {
	it('runs on the server, with the pictures it returns attached, and shows as what it does', async () => {
		const family = await connected();
		const chat = chatAsking(
			family,
			modelReply(
				[
					call('m1', 'mcp__fake__echo', { text: 'hi', shout: true }),
					call('m2', 'mcp__fake__picture')
				],
				'tool_use'
			),
			said('Done.')
		);

		await run(chat.id);

		const [echo, picture] = results(chat.id);
		expect(echo).toEqual({ type: 'tool_result', callId: 'm1', content: 'HI', isError: false });
		expect(picture).toMatchObject({ callId: 'm2', isError: false });
		expect(picture.content[0].text).toMatch(
			/^Here it is\.\nAttached \/.*image-1\.png \(1×1 PNG\)\.$/
		);
		expect(picture.content.some((b: { type: string }) => b.type === 'image')).toBe(true);
		// Its next request carries both results.
		expect(vi.mocked(streamTurn).mock.calls[1][0].tools).toEqual(chat.tools);

		const reply = committedRows(chat.id).find((row) => row.kind === 'assistant')!;
		const shown = toDisplay(reply);
		expect(shown.kind === 'assistant' && shown.blocks).toEqual([
			{
				type: 'tool',
				id: 'm1',
				command: 'fake echo {"text":"hi","shout":true}',
				summary: 'Echo (fake)',
				icon: 'plug'
			},
			{
				type: 'tool',
				id: 'm2',
				command: 'fake picture {}',
				summary: 'Picture (fake)',
				icon: 'plug'
			}
		]);
	});

	it('answers with what went wrong: a tool that reports an error, a server that is gone', async () => {
		const family = await connected();
		const chat = chatAsking(family);
		vi.mocked(streamTurn)
			.mockResolvedValueOnce(
				modelReply([call('m1', 'mcp__fake__fail'), call('m2', 'mcp__fake__echo', [1])], 'tool_use')
			)
			// An admin disconnects the server while the model writes its next call.
			.mockImplementationOnce(async () => {
				removeMcpServer('fake');
				return modelReply([call('m3', 'mcp__fake__echo', { text: 'x' })], 'tool_use');
			})
			.mockResolvedValueOnce(said('It broke.'));

		await run(chat.id);

		const [failed, invalid, gone] = results(chat.id);
		expect(failed).toEqual({
			type: 'tool_result',
			callId: 'm1',
			content: 'it broke',
			isError: true
		});
		expect(invalid).toMatchObject({
			callId: 'm2',
			content: 'Invalid input: the arguments must be an object.',
			isError: true
		});
		expect(gone).toMatchObject({ callId: 'm3', isError: true });
		expect(gone.content).toMatch(/^Not run: There's no MCP server called fake\./);
	});
});

describe('auto mode', () => {
	it('checks a call to a server’s tool, telling the check what the server says of it', async () => {
		const family = await connected();
		saveCommandMode('auto');
		const chat = chatAsking(
			family,
			modelReply([call('m1', 'mcp__fake__echo', { text: 'hi' })], 'tool_use'),
			modelReply([call('m2', 'mcp__fake__structured')], 'tool_use'),
			said('Done.')
		);
		vi.mocked(createMessage)
			.mockResolvedValueOnce(said('ALLOW'))
			.mockResolvedValueOnce(said('BLOCK'))
			.mockResolvedValueOnce(said('Verdict: BLOCK\nReason: Nobody asked to change anything.'));

		await run(chat.id);

		const [allowed, blocked] = results(chat.id);
		expect(allowed).toMatchObject({ callId: 'm1', content: 'hi', isError: false });
		expect(blocked).toMatchObject({ callId: 'm2', isError: true });
		expect(blocked.content).toMatch(/^Blocked by auto mode: Nobody asked to change anything\./);
		const first = vi.mocked(createMessage).mock.calls[0][0].input as string;
		expect(first).toContain(
			'A tool of the MCP server "fake", which the family connected (not a command)'
		);
		expect(first).toContain(
			'Tool: echo\nWhat the server says of it: it only reads\nArguments:\n{\n  "text": "hi"\n}'
		);
		expect(first).toContain('Message from Anna: Say hi through the fake server');
		const second = vi.mocked(createMessage).mock.calls[1][0].input as string;
		expect(second).toContain('Tool used: mcp__fake__echo {"text":"hi"}');
		expect(second).toContain('What the server says of it: it may delete or overwrite things');
	});
});
