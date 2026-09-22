<script lang="ts">
	// Requests this companion sent to paired companions (#110): the outbox
	// as GET /api/federation/outbox lists it, kept current by the
	// outbox_updated event, rendered by PeerRequestList. The section is
	// shown when there is something to show or when the listing failed;
	// an empty outbox is not a section.
	import { fetchFederationOutbox } from "$lib/api/client.js";
	import type { FederationOutboxEntry, ServerEvent } from "$lib/api/types.js";
	import { upsertOutboxEntry } from "$lib/activity/receipts.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import PeerRequestList from "$lib/components/activity/PeerRequestList.svelte";

	let { slug, now, limit = 50 }: { slug: string; now: number; limit?: number } = $props();

	const ws = getWebSocket();

	let entries = $state<FederationOutboxEntry[]>([]);
	let loading = $state(true);
	let loadError = $state("");

	const visible = $derived(entries.slice(0, limit));

	async function load() {
		loading = true;
		loadError = "";
		try {
			entries = (await fetchFederationOutbox()).entries;
		} catch {
			loadError = "Could not load the requests sent to companions.";
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
		const unsub = ws.subscribe((event: ServerEvent) => {
			if (event.type === "outbox_updated" && event.instance_slug === slug) {
				entries = upsertOutboxEntry(entries, event.entry);
			}
		});
		return unsub;
	});
</script>

{#if loadError}
	<section class="peer-section" aria-labelledby="peer-requests-heading">
		<div class="peer-head">
			<h3 id="peer-requests-heading">Sent to companions</h3>
		</div>
		<div class="peer-empty" role="alert"><p>{loadError}</p><button class="nl-button-secondary" onclick={load}>Try again</button></div>
	</section>
{:else if !loading && visible.length > 0}
	<section class="peer-section" aria-labelledby="peer-requests-heading">
		<div class="peer-head">
			<h3 id="peer-requests-heading">Sent to companions</h3>
			<p>Requests your companion sent to paired companions on your behalf: what it asked, where each one stands, how often it was tried, and what came back. Their companion decides by its own owner's policy.</p>
		</div>
		<PeerRequestList entries={visible} {now} />
	</section>
{/if}

<style>
	.peer-section { max-width: 720px; margin: 0 auto 32px; display: flex; flex-direction: column; gap: 12px; }
	.peer-head h3 { font: 400 22px/1.2 var(--font-display); letter-spacing: -0.02em; color: var(--foreground); margin: 0 0 6px; }
	.peer-head p { font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); margin: 0; }
	.peer-empty { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 24px 16px; text-align: center; color: var(--text-secondary); font: 400 14px/1.6 var(--font-body); background: var(--card); border: 1px solid var(--border); border-radius: 16px; margin: 0; }
	.peer-empty p { margin: 0; }
</style>
