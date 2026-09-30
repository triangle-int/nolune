import type {
	BackgroundItem,
	ChatModel,
	DisplayMemoryLook,
	DisplayMessage,
	LiveBlock,
	LiveEvent,
	Snapshot
} from '@nolune/core';
import type { ToolResult } from './transcript';

type ServerEvent = LiveEvent | { type: 'snapshot'; snapshot: Snapshot };

/** Live state of one conversation, fed by the server-sent event stream. */
export class ChatState {
	/** Empty until the first snapshot arrives. */
	title = $state('');
	/** The model and reasoning level, which anyone in the profile can change. Null until then. */
	model = $state<ChatModel | null>(null);
	messages = $state<DisplayMessage[]>([]);
	queued = $state<DisplayMessage[]>([]);
	running = $state(false);
	error = $state<string | null>(null);
	live = $state<(LiveBlock | null)[]>([]);
	toolOutput = $state<{ id: string; text: string } | null>(null);
	/** Background commands and subagents still working for this chat. */
	background = $state<BackgroundItem[]>([]);
	/** What the note-taker saved from this chat, each shown after the last message it read. */
	memory = $state<DisplayMemoryLook[]>([]);
	connected = $state(false);
	loaded = $state(false);

	/** Tool results keyed by tool_use id, so each command card can show its output. */
	results: Record<string, ToolResult> = $derived(
		Object.fromEntries(
			this.messages.flatMap((m) =>
				m.kind === 'tool_results' ? m.results.map((r) => [r.id, r] as const) : []
			)
		)
	);

	apply(event: ServerEvent) {
		switch (event.type) {
			case 'snapshot':
				this.title = event.snapshot.title;
				this.model = event.snapshot.model;
				this.messages = event.snapshot.messages;
				this.queued = event.snapshot.queued;
				this.running = event.snapshot.running;
				this.error = event.snapshot.error;
				this.live = event.snapshot.live;
				this.toolOutput = event.snapshot.toolOutput;
				this.background = event.snapshot.background;
				this.memory = event.snapshot.memory;
				this.loaded = true;
				break;
			case 'title':
				this.title = event.title;
				break;
			case 'model':
				this.model = event.model;
				break;
			case 'background':
				this.background = event.background;
				break;
			case 'memory':
				this.memory = event.memory;
				break;
			case 'status':
				this.running = event.running;
				this.error = event.error;
				break;
			case 'queued':
				this.queued = event.queued;
				break;
			case 'message':
				if (event.replacesLive) {
					this.live = [];
					this.toolOutput = null;
				}
				if (!this.messages.some((m) => m.id === event.message.id))
					this.messages.push(event.message);
				break;
			case 'live_block':
				this.live[event.index] = event.block;
				break;
			case 'live_delta': {
				const block = this.live[event.index];
				if (block) block.text += event.text;
				break;
			}
			case 'live_clear':
				this.live = [];
				this.toolOutput = null;
				break;
			case 'tool_output':
				if (this.toolOutput?.id !== event.id) this.toolOutput = { id: event.id, text: '' };
				this.toolOutput.text += event.chunk;
				break;
		}
	}

	/** Opens the event stream. EventSource reconnects by itself; each reconnect starts with a snapshot. */
	connect(conversationId: string): () => void {
		const source = new EventSource(`/api/c/${conversationId}/events`);
		source.onopen = () => (this.connected = true);
		source.onerror = () => (this.connected = false);
		source.onmessage = (event) => this.apply(JSON.parse(event.data) as ServerEvent);
		return () => source.close();
	}
}
