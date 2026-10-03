import type { DisplayMessage, DisplayResult } from './conversations.ts';
import type { DisplayMemoryLook } from './memory-changes.ts';
import type { LiveBlock, LiveEvent, Snapshot } from './runner.ts';
import { buildTranscript, type Entry, type ToolResult } from './transcript.ts';

/**
 * A chat's transcript as the gateway keeps it for apps of their own (`/api/c/<id>/transcript`):
 * what a page's ChatState keeps from the event stream to build it, so an app gets it built.
 */
export class ChatView {
	private messages: DisplayMessage[];
	private live: (LiveBlock | null)[];
	private running: boolean;
	private memory: DisplayMemoryLook[];

	/** From the chat's snapshot, copied: its live blocks are the runner's own, which deltas grow. */
	constructor(snapshot: Snapshot) {
		const copy = structuredClone({
			messages: snapshot.messages,
			live: snapshot.live,
			memory: snapshot.memory
		});
		this.messages = copy.messages;
		this.live = copy.live;
		this.memory = copy.memory;
		this.running = snapshot.running;
	}

	/** Takes in one of the chat's events. True when the transcript changes with it. */
	apply(event: LiveEvent): boolean {
		switch (event.type) {
			case 'status': {
				const changed = this.running !== event.running;
				this.running = event.running;
				return changed;
			}
			case 'message': {
				if (event.replacesLive) this.live = [];
				if (this.messages.some((m) => m.id === event.message.id)) return !!event.replacesLive;
				this.messages.push(event.message);
				return true;
			}
			case 'live_block':
				// Copied too: the runner grows the block it sent, and the deltas say so again.
				while (this.live.length < event.index) this.live.push(null);
				this.live[event.index] = { ...event.block };
				return true;
			case 'live_delta': {
				const block = this.live[event.index];
				if (!block) return false;
				block.text += event.text;
				return true;
			}
			case 'live_clear':
				this.live = [];
				return true;
			case 'memory':
				this.memory = event.memory;
				return true;
			default:
				return false;
		}
	}

	/** The transcript, and the commands' results by tool call, which its command steps show. */
	transcript(): Transcript {
		const results: [string, DisplayResult][] = this.messages.flatMap((m) =>
			m.kind === 'tool_results' ? m.results.map((r) => [r.id, r] as [string, DisplayResult]) : []
		);
		return {
			entries: buildTranscript(this.messages, this.live, this.running, this.memory),
			results: Object.fromEntries(results)
		};
	}
}

export interface Transcript {
	entries: Entry[];
	results: Record<string, ToolResult>;
}

/**
 * What an app hasn't been sent yet: the entries' keys in order, the entries that are new or
 * changed (by key, which stays the same while a reply streams and once it's saved), and results
 * of commands it doesn't have. An entry missing from the order is gone.
 */
export interface TranscriptUpdate {
	order: string[];
	entries: Entry[];
	results: Record<string, ToolResult>;
}

/** Remembers what an app was sent, to send it only what changed. */
export class TranscriptDiff {
	private sent = new Map<string, string>();
	private order: string | null = null;
	private sentResults = new Set<string>();

	/** What changed since the last call, the whole transcript the first time; null for nothing. */
	next(transcript: Transcript): TranscriptUpdate | null {
		const now = new Map(transcript.entries.map((e) => [e.key, JSON.stringify(e)]));
		const entries = transcript.entries.filter((e) => this.sent.get(e.key) !== now.get(e.key));
		const order = transcript.entries.map((e) => e.key);
		const orderChanged = order.join('\n') !== this.order;
		const results = Object.fromEntries(
			Object.entries(transcript.results).filter(([id]) => !this.sentResults.has(id))
		);
		if (!entries.length && !orderChanged && !Object.keys(results).length) return null;
		this.sent = now;
		this.order = order.join('\n');
		for (const id of Object.keys(results)) this.sentResults.add(id);
		return { order, entries, results };
	}
}

/**
 * Events that only change the transcript. While a reply streams they come many a second, so the
 * transcript goes out at most every `throttleMs`; any other event first sends what's waiting,
 * then goes as it is, so an app sees them in order.
 */
const TRANSCRIPT_EVENTS = new Set<LiveEvent['type']>([
	'message',
	'live_block',
	'live_delta',
	'live_clear',
	'memory'
]);

/**
 * A chat for an app: its snapshot with the transcript built in place of its rows and live blocks
 * (`snapshot`), then what changes in it (`transcript`, a TranscriptUpdate), and the chat's other
 * events (its title, model, status, who's typing…) as the page gets them. Returns a stop.
 */
export function streamTranscript(
	snapshot: Snapshot,
	subscribe: (listener: (event: LiveEvent) => void) => () => void,
	send: (data: unknown) => void,
	throttleMs = 100
): () => void {
	const view = new ChatView(snapshot);
	const diff = new TranscriptDiff();
	const first = diff.next(view.transcript());
	send({
		type: 'snapshot',
		snapshot: {
			title: snapshot.title,
			model: snapshot.model,
			commands: snapshot.commands,
			toolChanges: snapshot.toolChanges,
			running: snapshot.running,
			error: snapshot.error,
			queued: snapshot.queued,
			toolOutput: snapshot.toolOutput,
			background: snapshot.background,
			typing: snapshot.typing,
			entries: first?.entries ?? [],
			results: first?.results ?? {}
		}
	});

	let timer: ReturnType<typeof setTimeout> | null = null;
	const flush = () => {
		if (timer) clearTimeout(timer);
		timer = null;
		const update = diff.next(view.transcript());
		if (update) send({ type: 'transcript', ...update });
	};
	const unsubscribe = subscribe((event) => {
		const changed = view.apply(event);
		if (TRANSCRIPT_EVENTS.has(event.type)) {
			if (changed) timer ??= setTimeout(flush, throttleMs);
			return;
		}
		if (changed || timer) flush();
		send(event);
	});
	return () => {
		unsubscribe();
		if (timer) clearTimeout(timer);
	};
}
