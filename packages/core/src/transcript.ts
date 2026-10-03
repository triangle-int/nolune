import type { DisplayMessage, DisplayPicture, Usage } from './conversations.ts';
import type { DisplayMedia } from './media.ts';
import type { DisplayMemoryLook } from './memory-changes.ts';
import type { LiveBlock } from './runner.ts';

/**
 * Turns the stored rows into what the chat shows: people's messages, and nolune's replies as a run
 * of text parts and "activity" parts. An activity part is every thinking and command block
 * between two pieces of text, folded into one collapsible group. What the note-taker saved goes
 * after the last message it read.
 */

export interface ToolResult {
	output: string;
	isError: boolean;
	/** What the command attached with `nolune view`, shown under its output. */
	pictures: DisplayPicture[];
}

export type Step =
	| { type: 'thinking'; text: string }
	/**
	 * The model summarized the conversation so far, which had grown too long for it, and goes on
	 * from the summary. Empty while it's writing.
	 */
	| { type: 'compaction'; summary: string; asked: boolean }
	| {
			type: 'command';
			id: string;
			command: string | null;
			cwd?: string;
			/** What the model said the command does, in plain words. */
			summary: string | null;
			/** Lucide icon name the model picked. */
			icon: string | null;
	  };

export interface TextPart {
	type: 'text';
	key: string;
	text: string;
	/** Copies of the pictures and files the text links to, keyed by link target. */
	media?: Record<string, DisplayMedia>;
	/** Still streaming: its pictures and files aren't copied until the reply is saved. */
	pending?: boolean;
}

export interface ActivityPart {
	type: 'activity';
	key: string;
	steps: Step[];
	/** When the work started and last moved on, from row timestamps. Approximate. */
	startedAt: number;
	endedAt: number;
}

export type Part = TextPart | ActivityPart;

export interface Reply {
	type: 'reply';
	key: string;
	parts: Part[];
	/** Assistant rows in this reply, for usage and cache details. */
	messageIds: number[];
	usage: Usage | null;
	stopReasons: string[];
	/** The models that wrote it, in order: a chat can switch models, even in the middle of a turn. */
	models: string[];
	/** Still being written (the agent is running and this is the newest reply). */
	live: boolean;
}

export type Entry =
	| { type: 'human'; key: string; message: Extract<DisplayMessage, { kind: 'human' }> }
	| { type: 'trigger'; key: string; message: Extract<DisplayMessage, { kind: 'trigger' }> }
	| {
			type: 'agent_message';
			key: string;
			message: Extract<DisplayMessage, { kind: 'agent_message' }>;
	  }
	| { type: 'task_result'; key: string; message: Extract<DisplayMessage, { kind: 'task_result' }> }
	| { type: 'memory'; key: string; look: DisplayMemoryLook }
	/**
	 * A summary of the conversation after a reply, which someone asked for (`asked`) or the chat
	 * went quiet for, rather than one the model needed in the middle of its work (a step of that
	 * work). `live`: still being written.
	 */
	| { type: 'compaction'; key: string; summary: string; asked: boolean; live: boolean }
	| Reply;

/** Messages that aren't nolune's: each one ends the reply before it. */
function messageEntry(message: DisplayMessage): Entry | null {
	const key = `m${message.id}`;
	switch (message.kind) {
		case 'human':
			return { type: 'human', key, message };
		case 'trigger':
			return { type: 'trigger', key, message };
		case 'agent_message':
			return { type: 'agent_message', key, message };
		case 'task_result':
			return { type: 'task_result', key, message };
		default:
			return null;
	}
}

/** Sums usage, with what a summary of the conversation took in the call that wrote it. */
function addUsage(total: Usage | null, u: Usage | null): Usage | null {
	if (!u) return total;
	const sum: Usage = total ? { ...total } : { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 };
	for (const part of [u, u.compaction]) {
		if (!part) continue;
		sum.input += part.input;
		sum.cacheRead += part.cacheRead;
		sum.cacheWrite += part.cacheWrite;
		sum.output += part.output;
	}
	return sum;
}

export function buildTranscript(
	messages: DisplayMessage[],
	live: (LiveBlock | null)[],
	running: boolean,
	memory: DisplayMemoryLook[] = []
): Entry[] {
	const entries: Entry[] = [];
	let reply: Reply | null = null;
	/** Keys replies by what they answer, so a reply keeps its key when its live part is saved. */
	let anchor = 'start';
	let previousAt = 0;

	const looks = [...memory].sort((a, b) => a.after - b.after);
	/** The note-taker's looks at the chat before message `id`, each after the last one it read. */
	const addLooks = (id: number) => {
		while (looks.length && looks[0].after < id) {
			const look = looks.shift()!;
			const key = `memory-${look.after}`;
			entries.push({ type: 'memory', key, look });
			reply = null;
			anchor = key;
		}
	};

	const openReply = (): Reply => {
		if (!reply) {
			reply = {
				type: 'reply',
				key: `reply-${anchor}`,
				parts: [],
				messageIds: [],
				usage: null,
				stopReasons: [],
				models: [],
				live: false
			};
			entries.push(reply);
		}
		return reply;
	};

	const addStep = (r: Reply, step: Step, at: number) => {
		const last = r.parts.at(-1);
		if (last?.type === 'activity') {
			last.steps.push(step);
			last.endedAt = Math.max(last.endedAt, at);
		} else {
			r.parts.push({
				type: 'activity',
				key: `${r.key}-${r.parts.length}`,
				steps: [step],
				startedAt: previousAt || at,
				endedAt: at
			});
		}
	};

	const addText = (r: Reply, text: string, media: Pick<TextPart, 'media' | 'pending'>) => {
		r.parts.push({ type: 'text', key: `${r.key}-${r.parts.length}`, text, ...media });
	};

	let previousKind: DisplayMessage['kind'] | null = null;
	for (const message of messages) {
		addLooks(message.id);
		const entry = messageEntry(message);
		if (message.kind === 'compaction' && previousKind === 'assistant') {
			const key = `m${message.id}`;
			const asked = message.askedBy !== null;
			entries.push({ type: 'compaction', key, summary: message.summary, asked, live: false });
			reply = null;
			anchor = key;
		} else if (entry) {
			reply = null;
			anchor = String(message.id);
			entries.push(entry);
		} else if (message.kind === 'assistant') {
			const r = openReply();
			r.messageIds.push(message.id);
			r.usage = addUsage(r.usage, message.usage);
			if (message.stopReason) r.stopReasons.push(message.stopReason);
			if (message.model && r.models.at(-1) !== message.model) r.models.push(message.model);
			for (const block of message.blocks) {
				if (block.type === 'text') addText(r, block.text, { media: message.media });
				else if (block.type === 'thinking') addStep(r, block, message.createdAt);
				else if (block.type === 'compaction') {
					addStep(r, { ...block, asked: false }, message.createdAt);
				} else
					addStep(
						r,
						{
							type: 'command',
							id: block.id,
							command: block.command,
							cwd: block.cwd,
							summary: block.summary ?? null,
							icon: block.icon ?? null
						},
						message.createdAt
					);
			}
		} else if (message.kind === 'compaction') {
			// The runner had the model summarize the conversation before its next step.
			const r = openReply();
			r.usage = addUsage(r.usage, message.usage);
			const step = { type: 'compaction' as const, summary: message.summary };
			addStep(r, { ...step, asked: message.askedBy !== null }, message.createdAt);
		} else if (message.kind === 'tool_results' && reply) {
			// Command output arrived: the work that ends with these commands took until now.
			const last = (reply as Reply).parts.at(-1);
			if (last?.type === 'activity') last.endedAt = Math.max(last.endedAt, message.createdAt);
		}
		previousAt = message.createdAt;
		previousKind = message.kind;
	}

	addLooks(Infinity);

	const streaming = live.filter((b): b is LiveBlock => b !== null);
	if (
		previousKind === 'assistant' &&
		streaming.length === 1 &&
		streaming[0].type === 'compaction'
	) {
		// Someone asked for a summary after the reply: the model calls nothing else meanwhile.
		const summary = streaming[0].text;
		entries.push({
			type: 'compaction',
			key: 'compaction-live',
			summary,
			asked: false,
			live: running
		});
	} else if (streaming.length || running) {
		// While running, there is always a reply to show progress in, even before any output.
		const r = openReply();
		const now = Date.now();
		for (const block of streaming) {
			if (block.type === 'text') addText(r, block.text, { pending: true });
			else if (block.type === 'thinking') addStep(r, { type: 'thinking', text: block.text }, now);
			else if (block.type === 'compaction') {
				addStep(r, { type: 'compaction', summary: block.text, asked: false }, now);
			} else if (block.id)
				addStep(r, { type: 'command', id: block.id, ...partialToolInput(block.text) }, now);
		}
		r.live = running;
	}

	return entries;
}

/** The reply's visible text, for the copy button. */
export function replyText(reply: Reply): string {
	return reply.parts
		.flatMap((p) => (p.type === 'text' ? [p.text.trim()] : []))
		.join('\n\n')
		.trim();
}

/**
 * How a finished command ended. `blocked`: auto mode's check didn't let it run, and its result
 * says why (command-safety.ts in core).
 */
export function resultStatus(result: ToolResult): 'done' | 'failed' | 'stopped' | 'blocked' {
	if (!result.isError) return 'done';
	if (result.output.startsWith('Blocked by auto mode')) return 'blocked';
	return /Stopped by [^\n]*$/.test(result.output) ? 'stopped' : 'failed';
}

export interface ToolInput {
	command: string | null;
	/** Plain-language description the model writes with each call. */
	summary: string | null;
	/** Lucide icon name the model picked for the call. */
	icon: string | null;
}

/** Reads one string field out of tool input JSON that may still be streaming in. */
function partialString(json: string, key: string): string | null {
	const match = json.match(new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`));
	if (!match) return null;
	// The text may stop halfway through an escape like é; drop the unfinished tail.
	for (let cut = 0; cut <= 5; cut++) {
		try {
			return JSON.parse(`"${match[1].slice(0, match[1].length - cut)}"`) as string;
		} catch {
			// Try a shorter prefix.
		}
	}
	return match[1];
}

/** The fields of a `run_command` call whose input JSON is still streaming in. */
export function partialToolInput(json: string): ToolInput {
	let input: Record<string, unknown> | null = null;
	try {
		input = JSON.parse(json) as Record<string, unknown>;
	} catch {
		// Incomplete; read the fields one by one below.
	}
	const field = (key: string) => {
		const value = input ? input[key] : partialString(json, key);
		return typeof value === 'string' && value.trim() ? value.trim() : null;
	};
	// Only a finished icon name: "cloud" on the way to "cloud-sun" would flash the wrong icon.
	const icon = input ? field('icon') : (json.match(/"icon"\s*:\s*"([^"\\]+)"/)?.[1] ?? null);
	return { command: field('command'), summary: field('summary'), icon };
}
