<script lang="ts">
	// Requests that asked the owner (#109): presentational. Every label
	// comes from an ApprovalView built by lib/federation/policy.js, so the
	// /design-system sample and the live list cannot drift. A pending row
	// offers one bounded scope and Allow or Deny; a decided row can be taken
	// back. Nothing here ever shows what a peer sent: the server keeps the
	// intent and class, never a text.
	import SettingSelect from "$lib/components/settings/SettingSelect.svelte";
	import { approvalScopes, decisionErrorText, type ApprovalView } from "$lib/federation/policy.js";

	let {
		pending,
		decided = [],
		onapprove,
		ondeny,
		onwithdraw,
	}: {
		pending: ApprovalView[];
		decided?: ApprovalView[];
		/** Allows the request `id` within the chosen scope. Absent means read-only. */
		onapprove?: (id: string, scope: string) => Promise<void>;
		/** Denies the request `id` within the chosen scope. */
		ondeny?: (id: string, scope: string) => Promise<void>;
		/** Drops the entry `id` whatever it stands at. */
		onwithdraw?: (id: string) => Promise<void>;
	} = $props();

	const scopes = approvalScopes();
	let scopeById = $state<Record<string, string>>({});
	let busyId = $state("");
	let errorById = $state<Record<string, string>>({});

	function scopeFor(id: string): string {
		return scopeById[id] ?? "once";
	}

	async function act(id: string, run: () => Promise<void>) {
		if (busyId) return;
		busyId = id;
		errorById = { ...errorById, [id]: "" };
		try {
			await run();
		} catch (e) {
			errorById = { ...errorById, [id]: decisionErrorText(e) };
		} finally {
			busyId = "";
		}
	}
</script>

{#if pending.length > 0 || decided.length > 0}
	<div class="approvals" aria-live="polite">
		{#if pending.length > 0}
			<ul class="approvals-list" aria-label="Requests waiting for you">
				{#each pending as request (request.id)}
					<li class="approval" data-status={request.status}>
						<div class="approval-main">
							<p class="approval-text"><code class="approval-id" title={request.peerId}>{request.peerShortId}</code> wants to {request.label}.</p>
							<p class="approval-meta">{request.statusLabel} · {request.asked} · {request.expires}</p>
							{#if errorById[request.id]}<p class="approval-error" role="alert">{errorById[request.id]}</p>{/if}
						</div>
						{#if onapprove && ondeny}
							<div class="approval-actions">
								<div class="approval-scope">
									<span class="approval-scope-label" id={`approval-scope-${request.id}`}>Scope</span>
									<SettingSelect class="w-72 max-w-full" aria-labelledby={`approval-scope-${request.id}`} value={scopeFor(request.id)} options={scopes} disabled={busyId === request.id} onValueChange={(v) => (scopeById = { ...scopeById, [request.id]: v })} />
								</div>
								<button class="nl-button approval-primary" type="button" disabled={busyId === request.id} aria-label={`Allow ${request.peerShortId} to ${request.label}`} onclick={() => act(request.id, () => onapprove(request.id, scopeFor(request.id)))}>{busyId === request.id ? "Saving…" : "Allow"}</button>
								<button class="nl-button-secondary approval-secondary" type="button" disabled={busyId === request.id} aria-label={`Deny ${request.peerShortId} to ${request.label}`} onclick={() => act(request.id, () => ondeny(request.id, scopeFor(request.id)))}>Deny</button>
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
		{#if decided.length > 0}
			<ul class="approvals-list approvals-decided" aria-label="Decisions you can still take back">
				{#each decided as request (request.id)}
					<li class="approval" data-status={request.status}>
						<div class="approval-main">
							<p class="approval-text"><span class="approval-state">{request.statusLabel}</span> <code class="approval-id" title={request.peerId}>{request.peerShortId}</code> to {request.label}.</p>
							<p class="approval-meta">{request.status === "approved" ? "Waiting for it to ask again" : "It is refused until this lapses"} · {request.expires}</p>
							{#if errorById[request.id]}<p class="approval-error" role="alert">{errorById[request.id]}</p>{/if}
						</div>
						{#if onwithdraw}
							<div class="approval-actions">
								<button class="approval-link-btn" type="button" disabled={busyId === request.id} aria-label={`Withdraw the decision for ${request.peerShortId}`} onclick={() => act(request.id, () => onwithdraw(request.id))}>{busyId === request.id ? "Withdrawing…" : "Withdraw"}</button>
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
{/if}

<style>
	.approvals { display: flex; flex-direction: column; gap: 8px; margin: 4px 0 8px; }
	.approvals-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
	/* A request waiting for the owner sits on the selected surface so it reads as the thing to do. */
	/* The text keeps at least a readable column; the controls wrap under it when the row is narrow. */
	.approval { display: flex; flex-wrap: wrap; align-items: flex-start; justify-content: space-between; gap: 8px 12px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); min-width: 0; }
	.approval[data-status="pending"] { background: var(--accent); border-color: var(--primary); }
	.approval-main { display: flex; flex: 1 1 220px; flex-direction: column; gap: 4px; min-width: 0; }
	.approval-text { font: 400 14px/1.5 var(--font-body); color: var(--foreground); margin: 0; overflow-wrap: anywhere; }
	.approval-id { font: 500 13px/1.4 var(--font-mono, monospace); color: var(--foreground); }
	.approval-state { font: 500 13px/1.4 var(--font-body); color: var(--text-muted); padding: 2px 8px; border: 1px solid var(--border); border-radius: 999px; white-space: nowrap; }
	.approval[data-status="approved"] .approval-state { color: var(--primary); border-color: var(--primary); }
	.approval-meta { font: 400 13px/1.5 var(--font-body); color: var(--text-muted); margin: 0; overflow-wrap: anywhere; }
	.approval-error { font: 400 13px/1.5 var(--font-body); color: var(--destructive); margin: 2px 0 0; }
	.approval-actions { display: flex; flex: 0 1 auto; flex-wrap: wrap; align-items: end; gap: 8px; }
	.approval-scope { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
	.approval-scope-label { font: 500 12px/1.4 var(--font-body); color: var(--text-muted); letter-spacing: 0.03em; }
	.approval-primary { min-height: 44px; padding: 0 14px; font: 500 13px/1.5 var(--font-body); }
	.approval-secondary { min-height: 44px; }
	.approval-link-btn { min-height: 44px; padding: 0 12px; border: 1px solid transparent; border-radius: var(--radius-control, 8px); background: none; color: var(--text-secondary); font: 500 13px/1.5 var(--font-body); cursor: pointer; }
	.approval-link-btn:hover:not(:disabled) { color: var(--foreground); border-color: var(--border); }
	.approval-link-btn:disabled { opacity: 0.5; cursor: not-allowed; }
	@media (max-width: 480px) {
		.approval-actions { width: 100%; }
		.approval-scope { flex: 1 1 100%; }
	}
</style>
