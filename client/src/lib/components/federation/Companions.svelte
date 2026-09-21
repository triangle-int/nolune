<script lang="ts">
	// Companions (#108): the peer companions this server's owner paired
	// with, from GET /api/federation/peers, viewed by lib/federation/companions.js.
	// Invite mints a one-time line shown once in a panel that keeps nothing
	// after Done; Accept takes a pasted line (never a URL) and posts it in a
	// body; Confirm and Revoke act on a row; Rotate asks once, inline. The
	// listing is refreshed after every action and polled while shown, since
	// the other owner's confirmation lands on their server, not here.
	// Requests that asked the owner (#109) sit above the rows with Allow and
	// Deny within one bounded scope, and each paired row folds out what the
	// peer may do, one rule per pair, from GET /api/federation/policy and
	// GET /api/federation/approvals.
	import {
		acceptFederationInvite,
		approveFederationRequest,
		cancelFederationInvite,
		confirmFederationPeer,
		createFederationInvite,
		denyFederationRequest,
		fetchFederation,
		fetchFederationApprovals,
		fetchFederationPolicy,
		revokeFederationPeer,
		revokeFederationRule,
		rotateFederationIdentity,
		setFederationRule,
		withdrawFederationApproval,
		type FederationApproval,
		type FederationOverview,
		type FederationPolicyView,
	} from "$lib/api/client.js";
	import {
		acceptErrorText,
		inviteCountdown,
		inviteHandoff,
		looksLikeUrl,
		overviewView,
		rotationSummary,
		shortId,
		type InviteHandoff,
	} from "$lib/federation/companions.js";
	import { capabilityRows, decidedApprovals, pendingApprovals, scopeBody, type CapabilityRow } from "$lib/federation/policy.js";
	import CompanionRow from "./CompanionRow.svelte";
	import PeerCapabilities from "./PeerCapabilities.svelte";
	import PendingApprovals from "./PendingApprovals.svelte";

	const POLL_SECS = 30;

	let overview = $state<FederationOverview | null>(null);
	let policy = $state<FederationPolicyView | null>(null);
	let approvals = $state<FederationApproval[]>([]);
	let policyError = $state("");
	let loading = $state(true);
	let loadError = $state("");
	let now = $state(nowSeconds());

	/** The one-time invite panel; dropped on Done or expiry, and never stored anywhere else. */
	let handoff = $state<InviteHandoff | null>(null);
	let inviting = $state(false);
	let inviteError = $state("");
	let copied = $state(false);
	let cancellingId = $state("");

	let acceptOpen = $state(false);
	let acceptDraft = $state("");
	let accepting = $state(false);
	let acceptError = $state("");
	let acceptNotice = $state("");

	let rotateAsking = $state(false);
	let rotating = $state(false);
	let rotateError = $state("");
	let rotateNotice = $state("");
	let rotateButton = $state<HTMLButtonElement | null>(null);

	function nowSeconds(): number {
		return Math.floor(Date.now() / 1000);
	}

	const view = $derived(overview ? overviewView(overview, now) : null);
	const pending = $derived(pendingApprovals(approvals, now));
	const decided = $derived(decidedApprovals(approvals, now));
	function rowsFor(peerId: string): CapabilityRow[] {
		return policy ? capabilityRows(policy.defaults, policy.document.peers[peerId], now) : [];
	}
	const handoffLeft = $derived(handoff ? inviteCountdown(handoff.expiresAt, now) : "");
	const acceptLooksLikeUrl = $derived(looksLikeUrl(acceptDraft));

	$effect(() => {
		if (rotateAsking) rotateButton?.focus();
	});

	async function load(initial = false) {
		try {
			overview = await fetchFederation();
			loadError = "";
		} catch {
			if (initial || !overview) loadError = "Could not load companions.";
		} finally {
			loading = false;
		}
		await loadPolicy();
	}

	/** The rules and the queue, beside the listing; a failure here keeps the rows. */
	async function loadPolicy() {
		try {
			const [nextPolicy, queue] = await Promise.all([fetchFederationPolicy(), fetchFederationApprovals()]);
			policy = nextPolicy;
			approvals = queue.approvals ?? [];
			policyError = "";
		} catch {
			policyError = "Could not load what companions may do; the rows show the pairing only.";
		}
	}

	async function approveRequest(id: string, scope: string) {
		await approveFederationRequest(id, scopeBody(scope, nowSeconds()));
		await loadPolicy();
	}

	async function denyRequest(id: string, scope: string) {
		await denyFederationRequest(id, scopeBody(scope, nowSeconds()));
		await loadPolicy();
	}

	async function withdrawRequest(id: string) {
		await withdrawFederationApproval(id);
		await loadPolicy();
	}

	async function setRule(peerId: string, row: CapabilityRow, access: "allow" | "ask" | "deny") {
		await setFederationRule(peerId, { intent: row.intent, disclosure: row.disclosure, access });
		await loadPolicy();
	}

	async function revokeRule(peerId: string, row: CapabilityRow) {
		await revokeFederationRule(peerId, row.intent, row.disclosure);
		await loadPolicy();
	}

	async function invite() {
		inviting = true;
		inviteError = "";
		copied = false;
		try {
			// Only the line and its clock are kept from the response.
			handoff = inviteHandoff(await createFederationInvite());
			now = nowSeconds();
			await load();
		} catch {
			inviteError = "Could not create an invite. Check that this server is running.";
		} finally {
			inviting = false;
		}
	}

	async function copyLine() {
		if (!handoff) return;
		try {
			await navigator.clipboard.writeText(handoff.line);
			copied = true;
		} catch {
			copied = false;
			inviteError = "Could not copy; select the line and copy it yourself.";
		}
	}

	function doneWithInvite() {
		handoff = null;
		copied = false;
		load();
	}

	async function cancelInvite(id: string) {
		cancellingId = id;
		inviteError = "";
		try {
			await cancelFederationInvite(id);
			if (handoff?.id === id) handoff = null;
			await load();
		} catch {
			inviteError = "Could not withdraw that invite.";
		} finally {
			cancellingId = "";
		}
	}

	async function accept() {
		const line = acceptDraft.trim();
		if (!line || accepting || acceptLooksLikeUrl) return;
		accepting = true;
		acceptError = "";
		acceptNotice = "";
		try {
			const { peer } = await acceptFederationInvite(line);
			acceptDraft = "";
			acceptOpen = false;
			acceptNotice = `Accepted the invite from ${shortId(peer.companion_id)}. It pairs once its owner confirms on their server.`;
			await load();
		} catch (e) {
			acceptError = acceptErrorText(e);
		} finally {
			accepting = false;
		}
	}

	async function confirmPeer(id: string) {
		const { notified } = await confirmFederationPeer(id);
		await load();
		if (!notified) throw new Error("Paired here, but its server could not be reached; confirming again resends the notice.");
	}

	async function revokePeer(id: string) {
		const { notified } = await revokeFederationPeer(id);
		await load();
		if (!notified) throw new Error("Revoked here; its server could not be told, so it may still list this companion until its owner revokes too.");
	}

	async function rotate() {
		if (rotating) return;
		rotating = true;
		rotateError = "";
		rotateNotice = "";
		try {
			const report = await rotateFederationIdentity();
			rotateAsking = false;
			handoff = null;
			rotateNotice = rotationSummary(report);
			await load();
		} catch {
			rotateError = "Could not rotate the signing key; check the server log.";
		} finally {
			rotating = false;
		}
	}

	$effect(() => {
		load(true);
		const poll = setInterval(() => load(), POLL_SECS * 1000);
		const tick = setInterval(() => {
			now = nowSeconds();
			// An expired invite cannot be redeemed any more; the panel goes with it.
			if (handoff && handoff.expiresAt <= now) handoff = null;
		}, 1000);
		return () => {
			clearInterval(poll);
			clearInterval(tick);
		};
	});
</script>

{#if loading}
	<p class="dim-text">Loading...</p>
{:else if loadError || !view}
	<p class="setting-hint setting-warning" role="alert">{loadError || "Could not load companions."}</p>
	<div class="setting-input-row"><button class="setting-btn" onclick={() => { loading = true; load(true); }}>Retry</button></div>
{:else}
	<p class="setting-hint">This companion is <code class="companions-id" title={view.companionId}>{shortId(view.companionId)}</code>{#if view.rotations > 0}{" "}(its key was rotated {view.rotations === 1 ? "once" : `${view.rotations} times`}){/if}. Peers trust that id and its key, never this address or profile.</p>

	{#if pending.length > 0}
		<p class="setting-hint">A paired companion may only check that it can reach this one. Anything else asks you first, here; allow it once, for a while, or for that kind of request, or deny it. Nothing it sent is shown or kept.</p>
	{/if}
	<PendingApprovals {pending} {decided} onapprove={approveRequest} ondeny={denyRequest} onwithdraw={withdrawRequest} />
	{#if policyError}<p class="key-error" role="alert">{policyError}</p>{/if}

	{#if view.peers.length === 0}
		<p class="setting-hint">No companions are paired yet. Invite one, or accept an invite another owner gave you.</p>
	{:else}
		<ul class="companions-list" aria-label="Paired companions">
			{#each view.peers as peer (peer.id)}
				<CompanionRow {peer} onconfirm={peer.canConfirm ? () => confirmPeer(peer.id) : undefined} onrevoke={peer.canRevoke ? () => revokePeer(peer.id) : undefined}>
					{#if peer.state === "paired" && policy}
						<PeerCapabilities peerShortId={peer.shortId} rows={rowsFor(peer.id)} onset={(row, access) => setRule(peer.id, row, access)} onrevoke={(row) => revokeRule(peer.id, row)} />
					{/if}
				</CompanionRow>
			{/each}
		</ul>
	{/if}

	{#if view.invites.length > 0}
		<ul class="companions-invites" aria-label="Outstanding invites">
			{#each view.invites as invite (invite.id)}
				<li class="companions-invite">
					<span>{invite.text}. Its line was shown once when minted.</span>
					<button class="companion-link-btn" type="button" onclick={() => cancelInvite(invite.id)} disabled={cancellingId === invite.id}>{cancellingId === invite.id ? "Withdrawing…" : "Withdraw"}</button>
				</li>
			{/each}
		</ul>
	{/if}

	{#if handoff}
		<div class="pairing-panel companions-handoff" aria-live="polite">
			<p class="setting-hint">Hand this line to the other owner out of band (never in a URL). It works once and expires in <strong>{handoffLeft}</strong>. They paste it under Settings › Connections › Companions on their server, or run <code>nolune federation accept</code> there; then confirm their companion here.</p>
			<textarea class="companions-line" readonly rows="4" aria-label="Invite line" spellcheck="false" onfocus={(e) => e.currentTarget.select()}>{handoff.line}</textarea>
			<div class="setting-input-row">
				<button class="setting-btn" type="button" onclick={copyLine}>{copied ? "Copied" : "Copy line"}</button>
				<button class="nl-button-secondary" type="button" onclick={doneWithInvite}>Done</button>
				<button class="companion-link-btn" type="button" onclick={() => cancelInvite(handoff!.id)} disabled={cancellingId === handoff.id}>Withdraw</button>
			</div>
		</div>
	{:else if acceptOpen}
		<form class="companions-accept" onsubmit={(e) => { e.preventDefault(); accept(); }}>
			<label class="setting-label" for="companion-invite-line">Invite line from the other owner</label>
			<textarea id="companion-invite-line" class="companions-line" rows="4" bind:value={acceptDraft} placeholder="nolune-invite-v1.…" spellcheck="false" disabled={accepting}></textarea>
			{#if acceptLooksLikeUrl}<p class="key-error" role="alert">An invite is never a URL. Paste the nolune-invite line itself.</p>{/if}
			<div class="setting-input-row">
				<button class="setting-btn" type="submit" disabled={accepting || acceptLooksLikeUrl || !acceptDraft.trim()}>{accepting ? "Accepting…" : "Accept invite"}</button>
				<button class="nl-button-secondary" type="button" onclick={() => { acceptOpen = false; acceptDraft = ""; acceptError = ""; }} disabled={accepting}>Cancel</button>
			</div>
		</form>
	{:else if rotateAsking}
		<div class="companions-rotate" role="group" aria-labelledby="companions-rotate-label">
			<p id="companions-rotate-label" class="setting-hint">Rotate this companion's signing key? It gets a new key and id, outstanding invites and pending pairings are withdrawn, and every paired companion is told with a notice signed by the current key. Peers that cannot be reached keep trusting the old key until they hear the proof.</p>
			<div class="setting-input-row">
				<button bind:this={rotateButton} class="setting-btn setting-btn-danger" type="button" onclick={rotate} disabled={rotating} onkeydown={(e) => { if (e.key === "Escape") rotateAsking = false; }}>{rotating ? "Rotating…" : "Rotate key"}</button>
				<button class="nl-button-secondary" type="button" onclick={() => (rotateAsking = false)} disabled={rotating} onkeydown={(e) => { if (e.key === "Escape") rotateAsking = false; }}>Keep</button>
			</div>
		</div>
	{:else}
		<div class="setting-input-row">
			<button class="setting-btn" type="button" onclick={invite} disabled={inviting}>{inviting ? "..." : "Invite a companion"}</button>
			<button class="nl-button-secondary" type="button" onclick={() => { acceptOpen = true; acceptNotice = ""; }}>Accept an invite</button>
			<button class="nl-button-secondary" type="button" onclick={() => { rotateAsking = true; rotateNotice = ""; rotateError = ""; }}>Rotate signing key</button>
		</div>
	{/if}

	{#if inviteError}<p class="key-error" role="alert">{inviteError}</p>{/if}
	{#if acceptError}<p class="key-error" role="alert">{acceptError}</p>{/if}
	{#if acceptNotice}<p class="dim-text" role="status">{acceptNotice}</p>{/if}
	{#if rotateError}<p class="key-error" role="alert">{rotateError}</p>{/if}
	{#if rotateNotice}<p class="dim-text" role="status">{rotateNotice}</p>{/if}
{/if}

<style>
	.companions-id { font-family: var(--font-mono, monospace); color: var(--foreground); }
	.companions-list { list-style: none; margin: 4px 0 8px; padding: 0; display: flex; flex-direction: column; gap: 8px; }
	.companions-invites { list-style: none; margin: 4px 0 8px; padding: 0; display: flex; flex-direction: column; gap: 4px; font: 400 13px/1.5 var(--font-body); color: var(--text-muted); }
	.companions-invite { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 12px; }
	.companion-link-btn { min-height: 44px; padding: 0 12px; border: 1px solid transparent; border-radius: var(--radius-control, 8px); background: none; color: var(--text-secondary); font: 500 13px/1.5 var(--font-body); cursor: pointer; }
	.companion-link-btn:hover:not(:disabled) { color: var(--foreground); border-color: var(--border); }
	.companion-link-btn:disabled { opacity: 0.5; cursor: not-allowed; }
	.companions-handoff, .companions-accept, .companions-rotate { display: flex; flex-direction: column; gap: 8px; margin: 4px 0 8px; }
	.companions-accept, .companions-rotate { padding: 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--background); }
	.companions-handoff { align-items: stretch; }
	.companions-line { width: 100%; box-sizing: border-box; padding: 10px 12px; border: 1px solid var(--input); border-radius: 8px; background: var(--card); color: var(--foreground); font: 400 13px/1.5 var(--font-mono, monospace); overflow-wrap: anywhere; resize: vertical; }
	.companions-line:focus-visible { outline: 2px solid var(--ring); outline-offset: 1px; }
</style>
