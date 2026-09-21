<script lang="ts">
	// The federation inbox (#110): what paired companions asked this
	// companion for, as rows built by lib/federation/inbox.js from
	// GET /api/federation/inbox. A request that needs the owner sits first
	// and points at the Allow and Deny of its approval above; one the owner
	// already decided says so until the companion asks again; a settled one
	// says what happened and why (a denial by the owner once reads as
	// theirs, not the policy's). The two quoted labels are the peer's own
	// words (who it speaks for, why it asks), unverified; nothing a peer
	// sent beyond them is ever shown here, because the server keeps no text.
	import type { InboxRow } from "$lib/federation/inbox.js";

	let {
		rows,
		limit = 8,
	}: {
		rows: InboxRow[];
		/** How many settled rows to show under the ones that need the owner. */
		limit?: number;
	} = $props();

	const needing = $derived(rows.filter((row) => row.needsOwner));
	const settled = $derived(rows.filter((row) => !row.needsOwner).slice(0, limit));
</script>

{#if rows.length > 0}
	<div class="inbox" aria-live="polite">
		{#if needing.length > 0}
			<ul class="inbox-list" aria-label="Requests from companions waiting for you">
				{#each needing as row (row.key)}
					<li class="inbox-row" data-tone={row.tone}>
						<p class="inbox-text"><code class="inbox-id" title={row.peerId}>{row.peerShortId}</code> wants to {row.label}, speaking for <q class="inbox-words" title="The companion's own words, unverified">{row.representedOwner}</q>: <q class="inbox-words" title="The companion's own words, unverified">{row.purpose}</q></p>
						<p class="inbox-meta"><span class="inbox-state">{row.statusLabel}</span> · {row.when} · {row.note}</p>
					</li>
				{/each}
			</ul>
		{/if}
		{#if settled.length > 0}
			<ul class="inbox-list inbox-settled" aria-label="Recent requests from companions">
				{#each settled as row (row.key)}
					<li class="inbox-row" data-tone={row.tone}>
						<p class="inbox-text"><span class="inbox-state">{row.statusLabel}</span> <code class="inbox-id" title={row.peerId}>{row.peerShortId}</code> asked to {row.label}, speaking for <q class="inbox-words" title="The companion's own words, unverified">{row.representedOwner}</q>: <q class="inbox-words" title="The companion's own words, unverified">{row.purpose}</q></p>
						<p class="inbox-meta">{row.when} · {row.note}</p>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
{/if}

<style>
	.inbox { display: flex; flex-direction: column; gap: 8px; margin: 4px 0 8px; }
	.inbox-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
	.inbox-row { display: flex; flex-direction: column; gap: 4px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); min-width: 0; }
	/* A request waiting for the owner sits on the selected surface, beside the approval it waits on. */
	.inbox-row[data-tone="pending"] { background: var(--accent); border-color: var(--primary); }
	.inbox-text { font: 400 14px/1.5 var(--font-body); color: var(--foreground); margin: 0; overflow-wrap: anywhere; }
	.inbox-id { font: 500 13px/1.4 var(--font-mono, monospace); color: var(--foreground); }
	/* The peer's own words: quoted, in secondary text, never styled as a fact about the owner. */
	.inbox-words { font-style: italic; color: var(--text-secondary); quotes: "\201C" "\201D"; }
	.inbox-state { font: 500 13px/1.4 var(--font-body); color: var(--text-muted); padding: 2px 8px; border: 1px solid var(--border); border-radius: 999px; white-space: nowrap; }
	.inbox-row[data-tone="pending"] .inbox-state,
	.inbox-row[data-tone="approved"] .inbox-state { color: var(--primary); border-color: var(--primary); }
	.inbox-row[data-tone="denied"] .inbox-state { color: var(--destructive); border-color: var(--destructive); }
	.inbox-meta { font: 400 13px/1.5 var(--font-body); color: var(--text-muted); margin: 0; overflow-wrap: anywhere; }
</style>
