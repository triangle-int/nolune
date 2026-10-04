import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	BackgroundError,
	backgroundCommands,
	describeBackground,
	hasBackgroundCommands,
	listBackgroundCommands,
	processBackgroundStops,
	requestBackgroundStop,
	startBackgroundCommand,
	stopBackgroundCommands,
	type BackgroundCommand
} from './background.ts';
import { createConversation } from './conversations.ts';
import { runCommand, type RunCommandResult } from './run-command.ts';
import { findSubagent, runSubagent, setSubagentStatus } from './subagents.ts';
import { makeFamily, makePreset } from './test/fixtures.ts';

vi.mock('./run-command.ts', async (importOriginal) => ({
	...(await importOriginal<typeof import('./run-command.ts')>()),
	runCommand: vi.fn()
}));

const chats: string[] = [];

afterEach(() => {
	// What a test left running would stay in the gateway's list for the next.
	for (const id of chats.splice(0)) stopBackgroundCommands(id, 'the test');
	vi.resetAllMocks();
});

function chat(name = 'Anna'): string {
	const { user, profile } = makeFamily(name);
	const { id } = createConversation({
		profile,
		presetId: makePreset(`Sonnet for ${name}`).id,
		userId: user.id
	});
	chats.push(id);
	return id;
}

type Ended = { command: BackgroundCommand; result: RunCommandResult };

/** Starts a command that runs until it's stopped. `ended` resolves with how it ended. */
async function start(
	conversationId: string,
	pid: number,
	command: string,
	summary: string | null = null
): Promise<{ ended: Promise<Ended> }> {
	vi.mocked(runCommand).mockImplementationOnce((_input, options) => {
		options.onStart?.(pid);
		return new Promise((resolve) =>
			options.signal.addEventListener('abort', () =>
				resolve({ content: `[${options.abortReason()}]`, isError: true, exitCode: null })
			)
		);
	});
	let ended!: (end: Ended) => void;
	const end = new Promise<Ended>((resolve) => (ended = resolve));
	const outcome = await startBackgroundCommand({
		conversationId,
		toolUseId: `call-${pid}`,
		summary,
		input: { command, background: true },
		defaultCwd: '/',
		env: {},
		onEnd: (command, result) => ended({ command, result })
	});
	expect(outcome).toHaveProperty('started');
	return { ended: end };
}

const stoppedBy = (conversationId: string) =>
	Object.fromEntries(listBackgroundCommands(conversationId).map((c) => [c.pid, c.stoppedBy]));

describe('what runs in the background', () => {
	it("lists the conversation's commands and working subagents, by the ids that stop them", async () => {
		const id = chat();
		await start(id, 4242, 'fetch-photos --all\necho done', 'Downloading the photos');
		await start(chat('Ben'), 5151, 'sleep 100');
		const { subagent: flights } = runSubagent({ parentId: id, prompt: 'Find flights.\nCheap.' });
		setSubagentStatus(flights.id, 'running');
		const { subagent: done } = runSubagent({ parentId: id, prompt: 'Sort the photos.' });
		setSubagentStatus(done.id, 'done');
		runSubagent({ parentId: id, name: 'hotels', prompt: 'Find a hotel.' });

		expect(describeBackground(id, { now: Date.now() + 12 * 60_000 })).toEqual([
			'4242  command, running for 12 min: Downloading the photos',
			'  $ fetch-photos --all…',
			'agent-1  subagent, working for 12 min: Find flights.',
			'hotels  subagent, starting: Find a hotel.'
		]);
		expect(describeBackground(chat('Cleo'))).toEqual([]);
	});
});

describe('nolune background stop', () => {
	it('stops a command by its id within the gateway: its output is not handed over', async () => {
		const id = chat();
		const { ended } = await start(id, 4242, 'fetch-photos', 'Downloading the photos');
		await start(id, 4343, 'sleep 100');

		const asked = requestBackgroundStop(id, ['4242'], 'the agent');
		expect(asked.commands.map((c) => c.pid)).toEqual([4242]);
		expect(stoppedBy(id)).toEqual({ 4242: 'the agent', 4343: null });
		expect(describeBackground(id)[0]).toBe('4242  command, being stopped: Downloading the photos');

		processBackgroundStops();
		const { command, result } = await ended;
		expect(command.stoppedBy).toBe('the agent');
		expect(result.content).toBe('[Stopped by the agent.]');
		expect(listBackgroundCommands(id).map((c) => c.pid)).toEqual([4343]);
		expect(hasBackgroundCommands(id)).toBe(true);
	});

	it('leaves nothing to wait for once a stop is under way', async () => {
		const id = chat();
		const { ended } = await start(id, 4242, 'fetch-photos');
		requestBackgroundStop(id, ['4242'], 'the agent');
		expect(hasBackgroundCommands(id)).toBe(true);
		processBackgroundStops();
		// Still running as it ends, but an automation's run mustn't wait for output that won't come.
		expect(backgroundCommands(id).map((c) => c.pid)).toEqual([4242]);
		expect(hasBackgroundCommands(id)).toBe(false);
		await ended;
	});

	it('stops a subagent with the commands that only wait for it', async () => {
		const id = chat();
		const { subagent: flights } = runSubagent({ parentId: id, name: 'flights', prompt: 'Go.' });
		const { subagent: hotels } = runSubagent({ parentId: id, name: 'hotels', prompt: 'Go.' });
		setSubagentStatus(flights.id, 'running');
		setSubagentStatus(hotels.id, 'running');
		await start(id, 1, 'nolune agent watch flights');
		await start(id, 2, 'nolune agent watch flights; nolune agent watch hotels');
		await start(id, 3, 'fetch-photos');

		const asked = requestBackgroundStop(id, ['Flights'], 'the agent');
		expect(asked.subagents.map((s) => [s.name, s.status])).toEqual([['flights', 'stopping']]);
		expect(findSubagent(id, 'flights')?.error).toBe('Stopped by the agent that started it.');
		expect(stoppedBy(id)).toEqual({ 1: 'the agent', 2: null, 3: null });
	});

	it('stops everything with --all', async () => {
		const id = chat();
		await start(id, 1, 'nolune agent watch agent-1');
		await start(id, 2, 'fetch-photos');
		const { subagent } = runSubagent({ parentId: id, prompt: 'Go.' });
		setSubagentStatus(subagent.id, 'running');

		const asked = requestBackgroundStop(id, 'all', 'the agent');
		expect(asked.commands.map((c) => c.pid)).toEqual([1, 2]);
		expect(asked.subagents.map((s) => s.name)).toEqual(['agent-1']);
		expect(stoppedBy(id)).toEqual({ 1: 'the agent', 2: 'the agent' });
		expect(requestBackgroundStop(id, 'all', 'the agent')).toEqual({ commands: [], subagents: [] });
	});

	it("stops nothing when an id isn't running, and says why", async () => {
		const id = chat();
		await start(id, 4242, 'fetch-photos');
		const { subagent } = runSubagent({ parentId: id, prompt: 'Go.' });
		setSubagentStatus(subagent.id, 'done');

		expect(() => requestBackgroundStop(id, ['4242', 'nobody'], 'the agent')).toThrow(
			new BackgroundError(
				'nothing with the id "nobody" runs in this conversation\'s background. `nolune background` lists what does.'
			)
		);
		expect(() => requestBackgroundStop(id, ['agent-1'], 'the agent')).toThrow(
			"agent-1 isn't working (done)."
		);
		expect(stoppedBy(id)).toEqual({ 4242: null });
		// Another conversation's command isn't this one's to stop.
		expect(() => requestBackgroundStop(chat('Ben'), ['4242'], 'the agent')).toThrow(
			BackgroundError
		);

		requestBackgroundStop(id, ['4242'], 'the agent');
		expect(() => requestBackgroundStop(id, ['4242'], 'the agent')).toThrow(
			'4242 is being stopped already.'
		);
	});
});
