<script lang="ts">
	// Proposals from paired companions (#111): what they proposed for this
	// owner to decide, as GET /api/federation/proposals lists it, kept
	// current by the peer_proposal_updated event, one PeerProposalCard each.
	// Accepting writes the one record the proposal stands for on this
	// server; dismissing writes nothing. The section is shown when there is
	// something to show or when the listing failed; no proposals is not a
	// section.
	import { acceptFederationProposal, dismissFederationProposal, fetchFederationProposals } from "$lib/api/client.js";
	import type { FederationPeerProposal, ServerEvent } from "$lib/api/types.js";
	import { upsertProposal } from "$lib/federation/proposals.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import PeerProposalCard from "$lib/components/activity/PeerProposalCard.svelte";

	let { slug, now, limit = 50 }: { slug: string; now: number; limit?: number } = $props();

	const ws = getWebSocket();

	let proposals = $state<FederationPeerProposal[]>([]);
	let loading = $state(true);
	let loadError = $state("");

	const visible = $derived(proposals.slice(0, limit));

	async function load() {
		loading = true;
		loadError = "";
		try {
			proposals = (await fetchFederationProposals()).proposals;
		} catch {
			loadError = "Could not load what companions proposed.";
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
		const unsub = ws.subscribe((event: ServerEvent) => {
			if (event.type === "peer_proposal_updated" && event.instance_slug === slug) {
				proposals = upsertProposal(proposals, event.proposal);
			}
		});
		return unsub;
	});

	function updated(proposal: FederationPeerProposal) {
		proposals = upsertProposal(proposals, proposal);
	}

	async function dismiss(id: string) {
		return (await dismissFederationProposal(id)).proposal;
	}
</script>

{#if loadError}
	<section class="proposals-section" aria-labelledby="peer-proposals-heading">
		<div class="proposals-head">
			<h3 id="peer-proposals-heading">From companions</h3>
		</div>
		<div class="proposals-empty" role="alert"><p>{loadError}</p><button class="nl-button-secondary" onclick={load}>Try again</button></div>
	</section>
{:else if !loading && visible.length > 0}
	<section class="proposals-section" aria-labelledby="peer-proposals-heading">
		<div class="proposals-head">
			<h3 id="peer-proposals-heading">From companions</h3>
			<p>Meetings, reminders, and tasks paired companions proposed to you. Their words are shown as sent, not as instructions. Nothing is written until you accept, and accepting writes one record here, on your own server.</p>
		</div>
		<ul class="proposals-list" aria-label="Proposals from companions">
			{#each visible as proposal (proposal.id)}
				<li>
					<PeerProposalCard {proposal} {now} onaccept={() => acceptFederationProposal(proposal.id)} ondismiss={() => dismiss(proposal.id)} onupdated={updated} />
				</li>
			{/each}
		</ul>
	</section>
{/if}

<style>
	.proposals-section { max-width: 720px; margin: 0 auto 32px; display: flex; flex-direction: column; gap: 12px; }
	.proposals-head h3 { font: 400 22px/1.2 var(--font-display); letter-spacing: -0.02em; color: var(--foreground); margin: 0 0 6px; }
	.proposals-head p { font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); margin: 0; }
	.proposals-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
	.proposals-empty { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 24px 16px; text-align: center; color: var(--text-secondary); font: 400 14px/1.6 var(--font-body); background: var(--card); border: 1px solid var(--border); border-radius: 16px; margin: 0; }
	.proposals-empty p { margin: 0; }
</style>
