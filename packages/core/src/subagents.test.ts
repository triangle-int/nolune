import { describe, expect, it } from 'vitest';
import {
	appendRow,
	committedRows,
	createConversation,
	deleteConversation,
	foundText,
	getConversation,
	insertQueued,
	queuedRows
} from './conversations.ts';
import { TOOLS } from './run-command.ts';
import {
	MAX_ACTIVE_SUBAGENTS,
	SubagentError,
	findSubagent,
	requestSubagentStop,
	runSubagent,
	setSubagentStatus,
	settleSubagent,
	steerSubagent,
	subagentLogPath,
	subagentResult
} from './subagents.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

/** A chat where Anna already talked with btw. */
function parentChat() {
	const { user, profile } = makeFamily();
	const chat = createConversation({
		profile,
		presetId: makePreset().id,
		userId: user.id,
		effort: 'high'
	});
	insertQueued({
		conversationId: chat.id,
		senderId: user.id,
		senderName: 'Anna',
		text: 'We fly to Lisbon in October.'
	});
	return { chat, profile };
}

function reply(conversationId: string, text: string) {
	appendRow({
		conversationId,
		role: 'assistant',
		kind: 'assistant',
		content: JSON.stringify([
			{ type: 'thinking', thinking: 'secret', signature: 'x' },
			{ type: 'text', text }
		])
	});
}

describe('btw agent run', () => {
	it('starts a hidden conversation with only the task, a 5-minute cache and the chat’s model', () => {
		const { chat, profile } = parentChat();
		const { subagent, created } = runSubagent({ parentId: chat.id, prompt: 'Find flights.' });

		expect(created).toBe(true);
		expect(subagent).toMatchObject({ name: 'agent-1', status: 'pending', parentId: chat.id });
		const conv = getConversation(subagent.conversationId)!;
		expect(conv).toMatchObject({
			hidden: true,
			cacheTtl: '5m',
			model: chat.model,
			presetName: chat.presetName,
			effort: 'high',
			tools: TOOLS,
			profileId: profile.id
		});
		// Built for it, not copied: nothing of the parent's conversation comes along.
		expect(conv.systemPrompt).toContain('You are btw');
		const [task] = queuedRows(conv.id);
		expect(task).toMatchObject({
			kind: 'agent_message',
			senderName: 'agent-1',
			text: 'Find flights.'
		});
		expect(JSON.parse(task.content)[0].text).toContain('Find flights.');
		expect(JSON.stringify(queuedRows(conv.id))).not.toContain('Lisbon');
		expect(subagentLogPath(subagent)).toMatch(
			new RegExp(`/profiles/${profile.slug}/agents/${chat.id.slice(0, 8)}/agent-1\\.log$`)
		);
	});

	it('numbers subagents, or takes the id the agent gives', () => {
		const { chat } = parentChat();
		runSubagent({ parentId: chat.id, prompt: 'One' });
		expect(runSubagent({ parentId: chat.id, name: 'Flights', prompt: 'Two' }).subagent.name).toBe(
			'flights'
		);
		expect(runSubagent({ parentId: chat.id, prompt: 'Three' }).subagent.name).toBe('agent-3');
		expect(() => runSubagent({ parentId: chat.id, name: 'no spaces', prompt: 'x' })).toThrow(
			SubagentError
		);
	});

	it('gives a subagent that finished more work in the same conversation', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({
			parentId: chat.id,
			name: 'flights',
			prompt: 'Find flights.'
		});
		expect(() => runSubagent({ parentId: chat.id, name: 'flights', prompt: 'More' })).toThrow(
			/still working/
		);

		setSubagentStatus(subagent.id, 'done');
		const again = runSubagent({ parentId: chat.id, name: 'flights', prompt: 'Now trains.' });
		expect(again).toMatchObject({ created: false, subagent: { status: 'pending' } });
		expect(again.subagent.conversationId).toBe(subagent.conversationId);
		expect(queuedRows(subagent.conversationId).map((r) => r.text)).toEqual([
			'Find flights.',
			'Now trains.'
		]);
	});

	it(`allows ${MAX_ACTIVE_SUBAGENTS} working at once`, () => {
		const { chat } = parentChat();
		for (let i = 0; i < MAX_ACTIVE_SUBAGENTS; i++) runSubagent({ parentId: chat.id, prompt: 'x' });
		expect(() => runSubagent({ parentId: chat.id, prompt: 'x' })).toThrow(/at once/);
	});

	it('refuses subagents of subagents', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'x' });
		expect(() => runSubagent({ parentId: subagent.conversationId, prompt: 'y' })).toThrow(
			/subagents of its own/
		);
	});
});

describe('steering and stopping', () => {
	it('queues a steer for a working subagent and marks it pending, so the gateway starts it', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'Find flights.' });
		setSubagentStatus(subagent.id, 'running');

		steerSubagent({ parentId: chat.id, name: 'agent-1', text: 'Mornings only.' });

		expect(findSubagent(chat.id, 'agent-1')?.status).toBe('pending');
		const steer = queuedRows(subagent.conversationId).at(-1)!;
		expect(steer.text).toBe('Mornings only.');
		expect(JSON.parse(steer.content)[0].text).toContain('while you work');
	});

	it('refuses to steer one that finished, and says how to give it more work', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'x' });
		setSubagentStatus(subagent.id, 'done');
		expect(() => steerSubagent({ parentId: chat.id, name: 'agent-1', text: 'y' })).toThrow(
			/btw agent run agent-1/
		);
		expect(() => steerSubagent({ parentId: chat.id, name: 'nobody', text: 'y' })).toThrow(
			/no subagent "nobody"/
		);
	});

	it('never marks one done over a steer that arrived as it finished', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'Find flights.' });
		setSubagentStatus(subagent.id, 'running');
		// Its loop ended with its task still queued, as when a steer arrives just then.
		expect(settleSubagent(subagent.conversationId, 'done', null)).toBe('again');
		expect(findSubagent(chat.id, 'agent-1')?.status).toBe('pending');

		setSubagentStatus(subagent.id, 'stopping', 'Stopped by Anna.');
		expect(settleSubagent(subagent.conversationId, 'done', null)).toBe('skipped');
	});

	it('asks the gateway to stop one', () => {
		const { chat } = parentChat();
		runSubagent({ parentId: chat.id, prompt: 'x' });
		expect(requestSubagentStop({ parentId: chat.id, name: 'agent-1' }).status).toBe('stopping');
		expect(findSubagent(chat.id, 'agent-1')?.status).toBe('stopping');
	});
});

describe('results', () => {
	it('is its last message once it is done, and nothing while it works', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'x' });
		expect(subagentResult(findSubagent(chat.id, 'agent-1')!)).toBeNull();

		reply(subagent.conversationId, 'Looking…');
		reply(subagent.conversationId, 'TAP at 7:05, 89 € each.');
		setSubagentStatus(subagent.id, 'done');
		expect(subagentResult(findSubagent(chat.id, 'agent-1')!)).toEqual({
			ok: true,
			text: 'TAP at 7:05, 89 € each.'
		});

		setSubagentStatus(subagent.id, 'failed', 'Rate limited by Anthropic. Try again shortly.');
		expect(subagentResult(findSubagent(chat.id, 'agent-1')!)).toEqual({
			ok: false,
			text: 'agent-1 failed: Rate limited by Anthropic. Try again shortly.\n\nIts last message:\nTAP at 7:05, 89 € each.'
		});
	});

	it("leaves another agent's words out of what counts as found", () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'Show https://x.example/a.png' });
		appendRow({
			conversationId: subagent.conversationId,
			role: 'user',
			kind: 'tool_results',
			content: JSON.stringify([
				{ type: 'tool_result', tool_use_id: 't', content: 'https://found.example/b.png' }
			])
		});
		const rows = [
			...queuedRows(subagent.conversationId),
			...committedRows(subagent.conversationId)
		];
		expect(foundText(rows)).toContain('found.example');
		expect(foundText(rows)).not.toContain('x.example');
	});

	it('deletes a chat’s subagents with it', () => {
		const { chat } = parentChat();
		const { subagent } = runSubagent({ parentId: chat.id, prompt: 'x' });
		deleteConversation(chat.id);
		expect(getConversation(subagent.conversationId)).toBeUndefined();
	});
});
