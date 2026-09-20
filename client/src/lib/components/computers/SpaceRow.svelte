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
	}: {
		space: SpaceView;
		compact?: boolean;
		/** Saves the user's name; `null` shows the hostname again. Absent means the row cannot be renamed. */
		onrename?: (name: string | null) => Promise<void>;
	} = $props();

	let editing = $state(false);
	let draft = $state("");
	let saving = $state(false);
	let error = $state("");
	let input = $state<HTMLInputElement | null>(null);

	// The field takes focus when it appears, so Rename is one click away from typing.
	$effect(() => {
		if (editing) input?.focus();
	});

	const hints = $derived(compact ? space.hints.filter((h) => h.level === "warn") : space.hints);
	const inputId = $derived(`space-name-${space.id}`);

	function startRename() {
		draft = space.customName ?? "";
		error = "";
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
		{#if !compact && space.kind === "desktop"}
			<ul class="space-facts">
				{#each space.permissions as permission (permission.key)}
					<li class:space-fact-blocking={permission.blocking}>{permission.label} {permission.stateLabel}</li>
				{/each}
				<li>{space.capabilities}</li>
				<li>{space.cua}</li>
			</ul>
		{/if}
		{#each hints as hint (hint.text)}
			<p class="space-hint" class:space-hint-info={hint.level === "info"}>{hint.text}</p>
		{/each}
		{#if error}<p class="space-error" role="alert">{error}</p>{/if}
	</div>
	{#if space.canRename && onrename && !editing}
		<button class="space-rename-btn" type="button" onclick={startRename} aria-label={`Rename ${space.name}`}>Rename</button>
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
	.space-rename-btn { min-height: 44px; padding: 0 12px; border: 1px solid transparent; border-radius: var(--radius-control, 8px); background: none; color: var(--text-secondary); font: 500 13px/1.5 var(--font-body); cursor: pointer; }
	.space-rename-btn:hover { color: var(--foreground); border-color: var(--border); }
	.space-rename { display: flex; flex-direction: column; gap: 6px; }
	.space-rename-label { font: 500 13px/1.4 var(--font-body); color: var(--text-secondary); }
	.space-rename-row { display: flex; flex-wrap: wrap; gap: 8px; }
	.space-rename-input { flex: 1 1 200px; min-width: 0; }
	.space-rename-hint { font: 400 12px/1.5 var(--font-body); color: var(--text-muted); margin: 0; }
	@media (max-width: 480px) {
		/* The glyph keeps its column; Rename moves under the text instead of squeezing it. */
		.space { grid-template-columns: 32px minmax(0, 1fr); }
		.space-rename-btn { grid-column: 2; justify-self: start; margin-left: -12px; }
	}
</style>
