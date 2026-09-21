<script lang="ts">
	// What one peer companion may do (#109): presentational. One row per
	// pair the server could ever allow, built by capabilityRows in
	// lib/federation/policy.js: the sentence, what applies now and where it
	// comes from, and a native select that writes one rule (Allowed, Asks
	// you, Denied) or takes it back (Default). The list is folded until the
	// owner opens it, so a row stays short.
	import { accessLabel, type CapabilityRow } from "$lib/federation/policy.js";
	import { decisionErrorText } from "$lib/federation/policy.js";

	let {
		peerShortId,
		rows,
		open = false,
		onset,
		onrevoke,
	}: {
		peerShortId: string;
		rows: CapabilityRow[];
		/** Whether the list starts unfolded (the /design-system sample does). */
		open?: boolean;
		/** Writes the rule `access` for `row`. Absent means read-only. */
		onset?: (row: CapabilityRow, access: "allow" | "ask" | "deny") => Promise<void>;
		/** Takes the rule for `row` back so the default applies again. */
		onrevoke?: (row: CapabilityRow) => Promise<void>;
	} = $props();

	// The prop seeds the fold once; the owner toggles it from there.
	// svelte-ignore state_referenced_locally
	let unfolded = $state(open);
	let busyKey = $state("");
	let errorByKey = $state<Record<string, string>>({});

	const setCount = $derived(rows.filter((row) => row.source === "rule").length);
	const summary = $derived(setCount === 0 ? "all defaults" : `${setCount} set by you`);
	const editable = $derived(!!onset && !!onrevoke);

	async function change(row: CapabilityRow, value: string) {
		if (!onset || !onrevoke || busyKey) return;
		busyKey = row.key;
		errorByKey = { ...errorByKey, [row.key]: "" };
		try {
			if (value === "allow" || value === "ask" || value === "deny") await onset(row, value);
			else if (row.source === "rule") await onrevoke(row);
		} catch (e) {
			errorByKey = { ...errorByKey, [row.key]: decisionErrorText(e) };
		} finally {
			busyKey = "";
		}
	}
</script>

<div class="capabilities">
	<button class="capabilities-toggle" type="button" aria-expanded={unfolded} aria-controls={`capabilities-${peerShortId}`} onclick={() => (unfolded = !unfolded)}>
		<span class="capabilities-chevron" aria-hidden="true">{unfolded ? "▾" : "▸"}</span>
		What it may do <span class="capabilities-summary">({summary})</span>
	</button>
	{#if unfolded}
		<ul id={`capabilities-${peerShortId}`} class="capabilities-list" aria-label={`What ${peerShortId} may do`}>
			{#each rows as row (row.key)}
				<li class="capability" data-access={row.effective}>
					<div class="capability-main">
						<p class="capability-text">It may <strong>{row.label}</strong></p>
						<p class="capability-meta"><span class="capability-state">{row.effectiveLabel}</span>{#if row.source === "default"}{" "}by default{/if}{#if row.note}{" "}· {row.note}{/if}</p>
						{#if errorByKey[row.key]}<p class="capability-error" role="alert">{errorByKey[row.key]}</p>{/if}
					</div>
					{#if editable}
						<label class="capability-control">
							<span class="capability-control-label">Rule</span>
							<select class="setting-input" value={row.source === "rule" ? row.effective : "default"} disabled={busyKey === row.key} aria-label={`Rule for: ${row.label}`} onchange={(e) => change(row, (e.currentTarget as HTMLSelectElement).value)}>
								<option value="default">Default ({accessLabel(row.defaultAccess)})</option>
								<option value="allow">Allowed</option>
								<option value="ask">Asks you</option>
								<option value="deny">Denied</option>
							</select>
						</label>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.capabilities { display: flex; flex-direction: column; gap: 6px; margin-top: 4px; min-width: 0; }
	.capabilities-toggle { display: inline-flex; align-items: center; gap: 6px; min-height: 44px; padding: 0 8px 0 0; border: 0; background: none; color: var(--text-secondary); font: 500 13px/1.5 var(--font-body); cursor: pointer; text-align: left; }
	.capabilities-toggle:hover { color: var(--foreground); }
	.capabilities-toggle:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; border-radius: 6px; }
	.capabilities-chevron { width: 1em; color: var(--text-muted); }
	.capabilities-summary { color: var(--text-muted); font-weight: 400; }
	.capabilities-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 10px; background: var(--background); }
	.capability { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 4px 12px; padding: 8px 12px; min-width: 0; }
	.capability + .capability { border-top: 1px solid var(--border); }
	.capability-main { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
	.capability-text { font: 400 14px/1.5 var(--font-body); color: var(--foreground); margin: 0; overflow-wrap: anywhere; }
	.capability-text strong { font-weight: 500; }
	.capability-meta { font: 400 13px/1.5 var(--font-body); color: var(--text-muted); margin: 0; overflow-wrap: anywhere; }
	/* The word carries the state; its color only follows it. */
	.capability-state { font-weight: 500; color: var(--text-secondary); }
	.capability[data-access="allow"] .capability-state { color: var(--primary); }
	.capability[data-access="deny"] .capability-state { color: var(--destructive); }
	.capability-error { font: 400 13px/1.5 var(--font-body); color: var(--destructive); margin: 2px 0 0; }
	.capability-control { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
	.capability-control-label { font: 500 12px/1.4 var(--font-body); color: var(--text-muted); letter-spacing: 0.03em; }
	.capability-control select { min-height: 44px; max-width: 100%; }
	@media (max-width: 480px) {
		.capability { grid-template-columns: minmax(0, 1fr); }
	}
</style>
