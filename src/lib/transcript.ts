import type { DisplayMedia, DisplayMessage, LiveBlock, Usage } from '@btw/core';
import { partialToolInput } from './commands';

/**
 * Turns the stored rows into what the chat shows: people's messages, and btw's replies as a run
 * of text parts and "activity" parts. An activity part is every thinking and command block
 * between two pieces of text, folded into one collapsible group.
 */

export interface ToolResult {
	output: string;
	isError: boolean;
}

export type Step =
	| { type: 'thinking'; text: string }
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
	/** Still being written (the agent is running and this is the newest reply). */
	live: boolean;
}

export type Entry =
	| { type: 'human'; key: string; message: Extract<DisplayMessage, { kind: 'human' }> }
	| { type: 'trigger'; key: string; message: Extract<DisplayMessage, { kind: 'trigger' }> }
	| Reply;

function addUsage(total: Usage | null, u: Usage | null): Usage | null {
	if (!u) return total;
	if (!total) return { ...u };
	return {
		input: total.input + u.input,
		cacheRead: total.cacheRead + u.cacheRead,
		cacheWrite: total.cacheWrite + u.cacheWrite,
		output: total.output + u.output
	};
}

export function buildTranscript(
	messages: DisplayMessage[],
	live: (LiveBlock | null)[],
	running: boolean
): Entry[] {
	const entries: Entry[] = [];
	let reply: Reply | null = null;
	/** Keys replies by what they answer, so a reply keeps its key when its live part is saved. */
	let anchor = 'start';
	let previousAt = 0;

	const openReply = (): Reply => {
		if (!reply) {
			reply = {
				type: 'reply',
				key: `reply-${anchor}`,
				parts: [],
				messageIds: [],
				usage: null,
				stopReasons: [],
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

	for (const message of messages) {
		if (message.kind === 'human' || message.kind === 'trigger') {
			reply = null;
			anchor = String(message.id);
			entries.push(
				message.kind === 'human'
					? { type: 'human', key: `m${message.id}`, message }
					: { type: 'trigger', key: `m${message.id}`, message }
			);
		} else if (message.kind === 'assistant') {
			const r = openReply();
			r.messageIds.push(message.id);
			r.usage = addUsage(r.usage, message.usage);
			if (message.stopReason) r.stopReasons.push(message.stopReason);
			for (const block of message.blocks) {
				if (block.type === 'text') addText(r, block.text, { media: message.media });
				else if (block.type === 'thinking') addStep(r, block, message.createdAt);
				else
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
		} else if (message.kind === 'tool_results' && reply) {
			// Command output arrived: the work that ends with these commands took until now.
			const last = (reply as Reply).parts.at(-1);
			if (last?.type === 'activity') last.endedAt = Math.max(last.endedAt, message.createdAt);
		}
		previousAt = message.createdAt;
	}

	const streaming = live.filter((b): b is LiveBlock => b !== null);
	if (streaming.length || running) {
		// While running, there is always a reply to show progress in, even before any output.
		const r = openReply();
		const now = Date.now();
		for (const block of streaming) {
			if (block.type === 'text') addText(r, block.text, { pending: true });
			else if (block.type === 'thinking') addStep(r, { type: 'thinking', text: block.text }, now);
			else if (block.id)
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

export function formatDuration(ms: number): string | null {
	const seconds = Math.round(ms / 1000);
	if (seconds < 1) return null;
	if (seconds < 60) return `${seconds}s`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
	return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** How a finished command ended. */
export function resultStatus(result: ToolResult): 'done' | 'failed' | 'stopped' {
	if (!result.isError) return 'done';
	return /Stopped by [^\n]*$/.test(result.output) ? 'stopped' : 'failed';
}
