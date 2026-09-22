<script lang="ts">
	// One proposal from a paired companion (#111): a meeting, a reminder,
	// or a task handed over, laid out like a handoff card (#82) for review.
	// Everything shown comes from the server's proposal record through the
	// pure helpers in lib/federation/proposals.js. The details are the
	// companion's own words and sit in the same dashed panel the chat uses
	// for untrusted peer content, as plain text, never markup. Accept and
	// Dismiss go through the callbacks, so the card renders in the
	// design-system gallery without a backend; nothing is written until
	// Accept succeeds, and accepting writes exactly one record on this server.
	import type { FederationPeerProposal, FederationProposalAccepted } from "$lib/api/types.js";
	import { handoffSections, proposalActions, proposalLabel, proposalNote, proposalStatus, proposalStatusLabel, proposalText, proposalWhen } from "$lib/federation/proposals.js";
	import { shortId } from "$lib/federation/companions.js";
	import { relativeTime } from "$lib/activity/receipts.js";
	import { ShieldAlert } from "@lucide/svelte";

	let {
		proposal,
		now,
		onaccept,
		ondismiss,
		onupdated,
	}: {
		proposal: FederationPeerProposal;
		now: number;
		onaccept: () => Promise<FederationProposalAccepted>;
		ondismiss: () => Promise<FederationPeerProposal>;
		onupdated?: (proposal: FederationPeerProposal) => void;
	} = $props();

	let pending = $state<"" | "accept" | "dismiss">("");
	let error = $state("");
	let notice = $state("");

	const status = $derived(proposalStatus(proposal, now));
	const actions = $derived(proposalActions(proposal, now));
	const text = $derived(proposalText(proposal));
	const sections = $derived(proposal.details.kind === "handoff" ? handoffSections(proposal.details.task) : []);
	const when = $derived(proposalWhen(proposal));
	/** Waiting, done, or over, for the status word's color and the card's border. */
	const tone = $derived(status === "open" ? "open" : status === "accepted" ? "done" : "over");

	async function accept() {
		pending = "accept";
		error = "";
		notice = "";
		try {
			const accepted = await onaccept();
			notice = accepted.already_accepted ? "Already accepted; nothing new was written." : "";
			onupdated?.(accepted.proposal);
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not accept the proposal.";
		} finally {
			pending = "";
		}
	}

	async function dismiss() {
		pending = "dismiss";
		error = "";
		notice = "";
		try {
			onupdated?.(await ondismiss());
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not dismiss the proposal.";
		} finally {
			pending = "";
		}
	}
</script>

<article class="proposal" data-tone={tone} aria-labelledby={`peer-proposal-${proposal.id}`}>
	<header class="proposal-head">
		<div class="proposal-row">
			<h3 id={`peer-proposal-${proposal.id}`}>{proposalLabel(proposal)}</h3>
			<span class="proposal-time">{relativeTime(proposal.received_at, now)}</span>
		</div>
		{#if when}<p class="proposal-when">{when}</p>{/if}
		<p class="proposal-meta">Speaking for {proposal.represented_owner} · {proposal.purpose}</p>
	</header>

	<div class="proposal-words" role="group" aria-label={`Untrusted content from companion ${proposal.sender}`}>
		<p class="proposal-words-head"><ShieldAlert size={14} aria-hidden="true" /><span>Their words · from companion <code title={proposal.sender}>{shortId(proposal.sender)}</code> · untrusted, shown as data</span></p>
		{#if sections.length > 0}
			{#each sections as section (section.title)}
				<section class="proposal-section">
					<h4>{section.title}</h4>
					<ul>
						{#each section.items as item, i (i)}
							<li>{#if section.code}<code>{item}</code>{:else}{item}{/if}</li>
						{/each}
					</ul>
				</section>
			{/each}
		{:else}
			<p class="proposal-text">{text || "(empty)"}</p>
		{/if}
	</div>

	<div class="proposal-row">
		<span class="proposal-status">{proposalStatusLabel(proposal, now)}</span>
		<span class="proposal-note">{proposalNote(proposal, now)}</span>
	</div>

	{#if actions.accept || actions.dismiss}
		<div class="proposal-controls">
			{#if actions.accept}
				<button class="nl-button" onclick={accept} disabled={pending !== ""}>
					{pending === "accept" ? "Accepting…" : proposal.details.kind === "handoff" ? "Accept as my task" : "Accept"}
				</button>
			{/if}
			{#if actions.dismiss}
				<button class="nl-button-secondary" onclick={dismiss} disabled={pending !== ""}>
					{pending === "dismiss" ? "Dismissing…" : "Dismiss"}
				</button>
			{/if}
		</div>
	{/if}

	{#if error}<p class="proposal-error" role="alert">{error}</p>{/if}
	{#if notice}<p class="proposal-muted" role="status">{notice}</p>{/if}
</article>

<style>
	.proposal { background: var(--card); border: 1px solid var(--border); border-radius: 16px; padding: 16px 20px; display: flex; flex-direction: column; gap: 10px; min-width: 0; scroll-margin-top: 16px; }
	/* A proposal that waits for the owner sits on the lavender border; one that is over on the destructive one. */
	.proposal[data-tone="open"] { border-color: var(--primary); }
	.proposal[data-tone="over"] { border-color: var(--destructive); }
	.proposal-head { display: flex; flex-direction: column; gap: 4px; }
	.proposal-head h3 { margin: 0; font: 500 18px/1.3 var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.proposal-row { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
	.proposal-time { font: 400 12px var(--font-body); color: var(--text-muted); }
	.proposal-when { margin: 0; font: 400 14px/1.5 var(--font-body); color: var(--foreground); }
	.proposal-meta { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-muted); overflow-wrap: anywhere; }
	/* The companion's words: the same dashed panel the conversation uses for untrusted peer content, plain text only. */
	.proposal-words { border: 1px dashed var(--input); border-radius: 12px; background: var(--background); padding: 10px 14px; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
	.proposal-words-head { display: flex; align-items: center; gap: 6px; margin: 0; font: 500 12px/1.4 var(--font-body); color: var(--text-muted); letter-spacing: 0.02em; }
	.proposal-words-head code, .proposal-section code { font: 400 13px var(--font-mono); color: var(--text-secondary); overflow-wrap: anywhere; }
	.proposal-text { margin: 0; font: 400 15px/1.7 var(--font-body); color: var(--foreground); white-space: pre-wrap; overflow-wrap: anywhere; }
	.proposal-section { display: flex; flex-direction: column; gap: 4px; }
	.proposal-section h4 { margin: 0; font: 500 12px/1.5 var(--font-body); letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-muted); }
	.proposal-section ul { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 2px; }
	.proposal-section li { font: 400 14px/1.5 var(--font-body); color: var(--foreground); white-space: pre-wrap; overflow-wrap: anywhere; }
	.proposal-status { font: 500 13px var(--font-body); color: var(--primary); }
	.proposal[data-tone="done"] .proposal-status { color: var(--text-secondary); }
	.proposal[data-tone="over"] .proposal-status { color: var(--destructive); }
	.proposal-note { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); overflow-wrap: anywhere; }
	.proposal-controls { display: flex; flex-wrap: wrap; gap: 8px; }
	.proposal-muted { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-muted); }
	.proposal-error { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--destructive); }
	@media (max-width: 720px) {
		.proposal { padding: 14px 16px; }
		.proposal-controls .nl-button, .proposal-controls .nl-button-secondary { flex: 1 1 calc(50% - 8px); }
	}
</style>
