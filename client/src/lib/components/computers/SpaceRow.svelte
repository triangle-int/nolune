<script lang="ts">
	// One connected space (#80): the server home or a desktop. Presentational;
	// every label comes from a SpaceView built by lib/computers/spaces.js, so
	// the /design-system sample and the live list cannot drift.
	import Monitor from "@lucide/svelte/icons/monitor";
	import type { SpaceView } from "$lib/computers/spaces.js";

	let {
		space,
		compact = false,
		onrename,
		onforget,
	}: {
		space: SpaceView;
		compact?: boolean;
		/** Saves the user's name; `null` shows the hostname again. Absent means the row cannot be renamed. */
		onrename?: (name: string | null) => Promise<void>;
		/** Drops an offline computer's record and name. Absent means the row cannot be forgotten. */
		onforget?: () => Promise<void>;
	} = $props();

	let editing = $state(false);
	let draft = $state("");
	let saving = $state(false);
	let confirming = $state(false);
	let forgetting = $state(false);
	let error = $state("");
	/** A change that closed a step on its own, announced without stealing focus. */
	let notice = $state("");
	let input = $state<HTMLInputElement | null>(null);
	let confirmButton = $state<HTMLButtonElement | null>(null);

	// The field takes focus when it appears, so Rename is one click away from typing;
	// the confirm step likewise, so Forget is one more Enter or one Escape away.
	$effect(() => {
		if (editing) input?.focus();
	});
	$effect(() => {
		if (confirming) confirmButton?.focus();
	});
	// A computer that reconnects while its confirm is open can no longer be
	// forgotten (the server would refuse); the step closes and says why.
	$effect(() => {
		if (confirming && !forgetting && !(space.canForget && onforget)) {
			confirming = false;
			// A refusal that arrived first already says so.
			if (!error) notice = `${space.name} is connected again, so it stays listed.`;
		}
	});

	const hints = $derived(compact ? space.hints.filter((h) => h.level === "warn") : space.hints);
	const facts = $derived(!compact && (space.permissions.length > 0 || space.capabilities !== "" || space.cua !== ""));
	const inputId = $derived(`space-name-${space.id}`);

	function startRename() {
		draft = space.customName ?? "";
		error = "";
		notice = "";
		confirming = false;
		editing = true;
	}

	function cancelRename() {
		editing = false;
		error = "";
	}

	async function saveRename() {
		if (!onrename || saving) return;
		saving = true;
		error = "";
		try {
			await onrename(draft.trim() || null);
			editing = false;
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not rename this computer.";
		} finally {
			saving = false;
		}
	}

	function startForget() {
		error = "";
		notice = "";
		editing = false;
		confirming = true;
	}

	function cancelForget() {
		confirming = false;
		error = "";
	}

	async function confirmForget() {
		if (!onforget || forgetting) return;
		forgetting = true;
		error = "";
		try {
			await onforget();
			confirming = false;
		} catch (e) {
			// The server answers 409 `machine_online` for a computer that reconnected meanwhile.
			error = e instanceof Error && e.message ? e.message : "Could not forget this computer.";
		} finally {
			forgetting = false;
		}
	}
</script>

<li class="space" class:space-home={space.kind === "home"} class:space-compact={compact} data-status={space.status}>
	<div class="space-glyph" aria-hidden="true">
		{#if space.kind === "home"}
			<img src="/skins/moon/character.svg" alt="" width="22" height="22" />
		{:else}
			<Monitor size={20} strokeWidth={1.75} />
		{/if}
	</div>
	<div class="space-main">
		{#if editing}
			<form class="space-rename" onsubmit={(e) => { e.preventDefault(); saveRename(); }}>
				<label class="space-rename-label" for={inputId}>Name for {space.hostname}</label>
				<div class="space-rename-row">
					<input
						id={inputId}
						bind:this={input}
						class="nl-input space-rename-input"
						type="text"
						maxlength="64"
						placeholder={space.hostname}
						bind:value={draft}
						disabled={saving}
						onkeydown={(e) => { if (e.key === "Escape") cancelRename(); }}
					/>
					<button class="nl-button" type="submit" disabled={saving}>{saving ? "Saving…" : "Save"}</button>
					<button class="nl-button-secondary" type="button" onclick={cancelRename} disabled={saving}>Cancel</button>
				</div>
				<p class="space-rename-hint">Leave it blank to show the hostname again.</p>
			</form>
		{:else}
			<div class="space-head">
				<span class="space-name">{space.name}</span>
				{#if space.kind === "home"}<span class="space-pill">This server</span>{/if}
				{#if space.customName}<span class="space-hostname">{space.hostname}</span>{/if}
				<span class="space-state">{space.stateLabel}</span>
			</div>
		{/if}
		<p class="space-meta">{space.meta}{space.lastSeen ? ` · ${space.lastSeen}` : ""}</p>
		{#if space.note}<p class="space-note">{space.note}</p>{/if}
		{#if facts}
			<ul class="space-facts">
				{#each space.permissions as permission (permission.key)}
					<li class:space-fact-blocking={permission.blocking}>{permission.label} {permission.stateLabel}</li>
				{/each}
				{#if space.capabilities}<li>{space.capabilities}</li>{/if}
				{#if space.cua}<li>{space.cua}</li>{/if}
			</ul>
		{/if}
		{#each hints as hint (hint.text)}
			<p class="space-hint" class:space-hint-info={hint.level === "info"}>{hint.text}</p>
		{/each}
		{#if confirming}
			<div class="space-confirm" role="group" aria-labelledby={`space-forget-${space.id}`}>
				<p id={`space-forget-${space.id}`} class="space-confirm-text">Forget {space.name}? Its record and name are dropped; if it connects again it is listed as new.</p>
				<div class="space-confirm-row">
					<button bind:this={confirmButton} class="nl-button-secondary space-confirm-btn" type="button" onclick={confirmForget} disabled={forgetting} onkeydown={(e) => { if (e.key === "Escape") cancelForget(); }}>{forgetting ? "Forgetting…" : "Forget"}</button>
					<button class="nl-button-secondary" type="button" onclick={cancelForget} disabled={forgetting} onkeydown={(e) => { if (e.key === "Escape") cancelForget(); }}>Keep</button>
				</div>
			</div>
		{/if}
		{#if error}<p class="space-error" role="alert">{error}</p>{/if}
		{#if notice}<p class="space-notice" role="status">{notice}</p>{/if}
	</div>
	{#if !editing && !confirming && ((space.canRename && onrename) || (space.canForget && onforget))}
		<div class="space-actions">
			{#if space.canRename && onrename}
				<button class="space-action-btn" type="button" onclick={startRename} aria-label={`Rename ${space.name}`}>Rename</button>
			{/if}
			{#if space.canForget && onforget}
				<button class="space-action-btn" type="button" onclick={startForget} aria-label={`Forget ${space.name}`}>Forget</button>
			{/if}
		</div>
	{/if}
</li>

<style>
	.space { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto; align-items: start; gap: 4px 12px; padding: 14px 16px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); min-width: 0; }
	.space-compact { padding: 12px 14px; }
	.space-home { background: var(--popover); }
	.space-glyph { display: flex; align-items: center; justify-content: center; width: 32px; height: 32px; border-radius: 8px; background: var(--background); color: var(--text-secondary); margin-top: 1px; }
	.space-home .space-glyph { background: var(--accent); }
	.space-main { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
	.space-head { display: flex; align-items: center; flex-wrap: wrap; gap: 6px 10px; min-width: 0; }
	.space-name { font: 500 15px/1.4 var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.space-hostname { font: 400 13px/1.4 var(--font-mono, monospace); color: var(--text-muted); overflow-wrap: anywhere; }
	.space-pill { font: 500 11px/1.4 var(--font-body); letter-spacing: 0.04em; text-transform: uppercase; padding: 2px 8px; border-radius: 999px; background: var(--accent); color: var(--primary); }
	/* Status colors always sit beside their word; the word is the signal. */
	.space-state { font: 500 13px/1.4 var(--font-body); color: var(--text-muted); padding: 2px 8px; border: 1px solid var(--border); border-radius: 999px; white-space: nowrap; }
	.space[data-status="online"] .space-state { color: var(--primary); border-color: var(--primary); }
	.space[data-status="unhealthy"] .space-state, .space[data-status="restricted"] .space-state { color: var(--destructive); border-color: var(--destructive); }
	.space-meta { font: 400 13px/1.5 var(--font-body); color: var(--text-muted); margin: 0; overflow-wrap: anywhere; }
	.space-note { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); margin: 0; }
	.space-facts { list-style: none; margin: 2px 0 0; padding: 0; display: flex; flex-wrap: wrap; gap: 4px 12px; font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); }
	.space-fact-blocking { color: var(--destructive); }
	.space-hint { font: 400 13px/1.5 var(--font-body); color: var(--foreground); margin: 2px 0 0; padding-left: 10px; border-left: 2px solid var(--destructive); }
	.space-hint-info { color: var(--text-muted); border-left-color: var(--border); }
	.space-error { font: 400 13px/1.5 var(--font-body); color: var(--destructive); margin: 2px 0 0; }
	.space-notice { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); margin: 2px 0 0; }
	.space-actions { display: flex; flex-wrap: wrap; gap: 4px; }
	.space-action-btn { min-height: 44px; padding: 0 12px; border: 1px solid transparent; border-radius: var(--radius-control, 8px); background: none; color: var(--text-secondary); font: 500 13px/1.5 var(--font-body); cursor: pointer; }
	.space-action-btn:hover { color: var(--foreground); border-color: var(--border); }
	.space-confirm { display: flex; flex-direction: column; gap: 8px; margin-top: 4px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 10px; background: var(--background); }
	.space-confirm-text { font: 400 13px/1.5 var(--font-body); color: var(--foreground); margin: 0; }
	.space-confirm-row { display: flex; flex-wrap: wrap; gap: 8px; }
	/* Destructive confirm follows the outlined pattern from settings: the word and its border carry the color. */
	.space-confirm-btn { background: none; border-color: var(--destructive); color: var(--destructive); }
	.space-confirm-btn:hover:not(:disabled) { background: var(--accent); border-color: var(--destructive); color: var(--destructive); }
	.space-rename { display: flex; flex-direction: column; gap: 6px; }
	.space-rename-label { font: 500 13px/1.4 var(--font-body); color: var(--text-secondary); }
	.space-rename-row { display: flex; flex-wrap: wrap; gap: 8px; }
	.space-rename-input { flex: 1 1 200px; min-width: 0; }
	.space-rename-hint { font: 400 12px/1.5 var(--font-body); color: var(--text-muted); margin: 0; }
	@media (max-width: 480px) {
		/* The glyph keeps its column; Rename and Forget move under the text instead of squeezing it. */
		.space { grid-template-columns: 32px minmax(0, 1fr); }
		.space-actions { grid-column: 2; justify-self: start; margin-left: -12px; }
	}
</style>
