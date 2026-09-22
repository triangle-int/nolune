<script lang="ts">
	// The rows of requests sent to paired companions (#110), from outbox
	// entries alone through the pure helpers in lib/activity/receipts.js:
	// what was asked of whom, this owner's own words as sent, where it
	// stands, how often it was tried, and what came back. The peer's typed
	// response carries ids, classes, times, and spans; no word of the
	// peer's is ever shown here. `PeerRequests.svelte` feeds it from the
	// API; `/design-system` from sample records.
	import type { FederationOutboxEntry } from "$lib/api/types.js";
	import { outboxLabel, outboxNote, outboxStatusLabel, outboxText, relativeTime } from "$lib/activity/receipts.js";

	let { entries, now }: { entries: FederationOutboxEntry[]; now: number } = $props();

	/** In progress, done, or over, for the status word's color and the card's border. */
	function tone(entry: FederationOutboxEntry): "open" | "done" | "over" {
		switch (entry.status) {
			case "queued":
			case "waiting_owner":
				return "open";
			case "delivered":
				return "done";
			default:
				return "over";
		}
	}
</script>

<ul class="peer-list" aria-label="Requests sent to companions">
	{#each entries as entry (entry.intent.correlation_id)}
		{@const state = tone(entry)}
		<li id={`peer-request-${entry.intent.correlation_id}`} class="peer-item" data-tone={state}>
			<div class="peer-row">
				<span class="peer-label">{outboxLabel(entry)}</span>
				<span class="peer-time">{relativeTime(entry.created_at, now)}</span>
			</div>
			{#if outboxText(entry)}
				<p class="peer-text">{outboxText(entry)}</p>
			{/if}
			<p class="peer-meta">Speaking for {entry.intent.represented_owner} · {entry.intent.purpose}</p>
			<div class="peer-row">
				<span class="peer-status">{outboxStatusLabel(entry)}</span>
				<span class="peer-note">{outboxNote(entry, now)}</span>
			</div>
		</li>
	{/each}
</ul>

<style>
	.peer-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
	.peer-item { background: var(--card); border: 1px solid var(--border); border-radius: 16px; padding: 16px 20px; display: flex; flex-direction: column; gap: 6px; scroll-margin-top: 16px; min-width: 0; }
	.peer-item:target { outline: 2px solid var(--ring); outline-offset: 2px; }
	/* A request still on its way sits on the lavender border; one that is over on the destructive one. */
	.peer-item[data-tone="open"] { border-color: var(--primary); }
	.peer-item[data-tone="over"] { border-color: var(--destructive); }
	.peer-row { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
	.peer-label { font: 500 14px var(--font-body); color: var(--foreground); }
	.peer-time { font: 400 12px var(--font-body); color: var(--text-muted); }
	/* This owner's own words, as sent: plain pre-wrapped text, never markup. */
	.peer-text { margin: 0; font: 400 14px/1.5 var(--font-body); color: var(--text-secondary); white-space: pre-wrap; overflow-wrap: anywhere; }
	.peer-meta { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-muted); overflow-wrap: anywhere; }
	.peer-status { font: 500 13px var(--font-body); color: var(--primary); }
	.peer-item[data-tone="done"] .peer-status { color: var(--text-secondary); }
	.peer-item[data-tone="over"] .peer-status { color: var(--destructive); }
	.peer-note { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); overflow-wrap: anywhere; }
	@media (max-width: 720px) { .peer-item { padding: 14px 16px; } }
</style>
