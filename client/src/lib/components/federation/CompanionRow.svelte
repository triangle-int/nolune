<script lang="ts">
	// One peer companion (#108): presentational. Every label comes from a
	// PeerView built by lib/federation/companions.js, so the /design-system
	// sample and the live list cannot drift. Confirm pairs a peer whose
	// owner redeemed this server's invite; Revoke asks once, inline.
	import type { Snippet } from "svelte";
	import type { PeerView } from "$lib/federation/companions.js";

	let {
		peer,
		onconfirm,
		onrevoke,
		children,
	}: {
		peer: PeerView;
		/** Pairs a pending peer this server invited. Absent means the row cannot confirm. */
		onconfirm?: () => Promise<void>;
		/** Withdraws trust and tells the peer. Absent means the row cannot revoke. */
		onrevoke?: () => Promise<void>;
		/** Rendered under the row's text: what the peer may do (#109). */
		children?: Snippet;
	} = $props();

	let confirming = $state(false);
	let revoking = $state(false);
	let asking = $state(false);
	let error = $state("");
	let notice = $state("");
	let revokeButton = $state<HTMLButtonElement | null>(null);

	// The confirm step takes focus when it appears, so Revoke is one more
	// Enter or one Escape away.
	$effect(() => {
		if (asking) revokeButton?.focus();
	});

	const canConfirm = $derived(peer.canConfirm && !!onconfirm);
	const canRevoke = $derived(peer.canRevoke && !!onrevoke);
	const origins = $derived(
		peer.approvedOrigins.length > 0
			? `reached at ${peer.approvedOrigins.join(", ")}`
			: peer.pendingOrigin
				? `reports ${peer.pendingOrigin}`
				: "no approved address",
	);

	async function confirm() {
		if (!onconfirm || confirming) return;
		confirming = true;
		error = "";
		notice = "";
		try {
			await onconfirm();
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not confirm this companion.";
		} finally {
			confirming = false;
		}
	}

	function startRevoke() {
		error = "";
		notice = "";
		asking = true;
	}

	function keep() {
		asking = false;
	}

	async function revoke() {
		if (!onrevoke || revoking) return;
		revoking = true;
		error = "";
		try {
			await onrevoke();
			asking = false;
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not revoke this companion.";
		} finally {
			revoking = false;
		}
	}
</script>

<li class="companion" data-tone={peer.tone}>
	<div class="companion-main">
		<div class="companion-head">
			<code class="companion-id" title={peer.id}>{peer.shortId}</code>
			<span class="companion-state">{peer.stateLabel}</span>
		</div>
		<p class="companion-meta">{peer.roleLabel} · {origins} · {peer.lastSeen}</p>
		{#if peer.note}<p class="companion-note">{peer.note}</p>{/if}
		{#if asking}
			<div class="companion-confirm" role="group" aria-labelledby={`companion-revoke-${peer.id}`}>
				<p id={`companion-revoke-${peer.id}`} class="companion-confirm-text">Revoke {peer.shortId}? Nothing it signs will verify here, its server is told, and only a new invite pairs it again.</p>
				<div class="companion-confirm-row">
					<button bind:this={revokeButton} class="nl-button-secondary companion-confirm-btn" type="button" onclick={revoke} disabled={revoking} onkeydown={(e) => { if (e.key === "Escape") keep(); }}>{revoking ? "Revoking…" : "Revoke"}</button>
					<button class="nl-button-secondary" type="button" onclick={keep} disabled={revoking} onkeydown={(e) => { if (e.key === "Escape") keep(); }}>Keep</button>
				</div>
			</div>
		{/if}
		{#if error}<p class="companion-error" role="alert">{error}</p>{/if}
		{#if notice}<p class="companion-notice" role="status">{notice}</p>{/if}
		{@render children?.()}
	</div>
	{#if !asking && (canConfirm || canRevoke)}
		<div class="companion-actions">
			{#if canConfirm}
				<button class="nl-button companion-primary" type="button" onclick={confirm} disabled={confirming} aria-label={`Confirm ${peer.shortId}`}>{confirming ? "Confirming…" : "Confirm"}</button>
			{/if}
			{#if canRevoke}
				<button class="companion-action-btn" type="button" onclick={startRevoke} aria-label={`Revoke ${peer.shortId}`}>Revoke</button>
			{/if}
		</div>
	{/if}
</li>

<style>
	.companion { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: 4px 12px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); min-width: 0; }
	.companion-main { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
	.companion-head { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 10px; min-width: 0; }
	.companion-id { font: 500 14px/1.4 var(--font-mono, monospace); color: var(--foreground); overflow-wrap: anywhere; }
	/* Status colors always sit beside their word; the word is the signal. */
	.companion-state { font: 500 13px/1.4 var(--font-body); color: var(--text-muted); padding: 2px 8px; border: 1px solid var(--border); border-radius: 999px; white-space: nowrap; }
	.companion[data-tone="online"] .companion-state { color: var(--primary); border-color: var(--primary); }
	.companion[data-tone="warn"] .companion-state { color: var(--foreground); border-color: var(--foreground); }
	.companion-meta { font: 400 13px/1.5 var(--font-body); color: var(--text-muted); margin: 0; overflow-wrap: anywhere; }
	.companion-note { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); margin: 0; overflow-wrap: anywhere; }
	.companion-error { font: 400 13px/1.5 var(--font-body); color: var(--destructive); margin: 2px 0 0; }
	.companion-notice { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); margin: 2px 0 0; }
	.companion-actions { display: flex; flex-wrap: wrap; gap: 4px; }
	.companion-primary { min-height: 44px; padding: 0 14px; font: 500 13px/1.5 var(--font-body); }
	.companion-action-btn { min-height: 44px; padding: 0 12px; border: 1px solid transparent; border-radius: var(--radius-control, 8px); background: none; color: var(--text-secondary); font: 500 13px/1.5 var(--font-body); cursor: pointer; }
	.companion-action-btn:hover { color: var(--foreground); border-color: var(--border); }
	.companion-confirm { display: flex; flex-direction: column; gap: 8px; margin-top: 4px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--background); }
	.companion-confirm-text { font: 400 13px/1.5 var(--font-body); color: var(--foreground); margin: 0; }
	.companion-confirm-row { display: flex; flex-wrap: wrap; gap: 8px; }
	/* Destructive confirm follows the outlined pattern from settings: the word and its border carry the color. */
	.companion-confirm-btn { background: none; border-color: var(--destructive); color: var(--destructive); }
	@media (max-width: 480px) {
		.companion { grid-template-columns: minmax(0, 1fr); }
	}
</style>
