<script module lang="ts">
	import type { MemoryFlags } from "$lib/api/types.js";

	/** What a control changed, so the surface around it can re-read the memory. */
	export type MemoryChange =
		| { kind: "corrected" }
		| { kind: "resolved"; keep: "current" | "proposed" }
		| { kind: "flagged"; flags: MemoryFlags }
		| { kind: "forgotten" };
</script>

<script lang="ts">
	/**
	 * Correction controls for one memory (#84): correct, pin, exclude from
	 * proactive use, forget, and a link to inspect it in the library. Every
	 * request goes through the companion-scoped api client. A correction that
	 * conflicts with an earlier one is never merged: both statements are shown
	 * and the user picks the one that stays.
	 */
	import { correctMemoryFile, deleteMemoryFile, fetchMemoryContent, resolveMemoryCorrection, setMemoryFlags } from "$lib/api/client.js";
	import type { CorrectionConflict } from "$lib/api/types.js";
	import { boundTextPath, canFlag, conflictPrompt, correctionOutcome, flagControls, mediaBoundText, memoryBody, recalledWhen } from "$lib/memory/receipts.js";
	import { getToasts } from "$lib/stores/toast.svelte.js";
	import Pencil from "@lucide/svelte/icons/pencil";
	import Pin from "@lucide/svelte/icons/pin";
	import PinOff from "@lucide/svelte/icons/pin-off";
	import Eye from "@lucide/svelte/icons/eye";
	import EyeOff from "@lucide/svelte/icons/eye-off";
	import Trash2 from "@lucide/svelte/icons/trash-2";
	import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";

	const toast = getToasts();

	let {
		slug,
		path,
		flags = null,
		inspectHref = "",
		onchange,
	}: {
		slug: string;
		path: string;
		/** Current flags of a text memory; `null` while unknown or for media. */
		flags?: MemoryFlags | null;
		/** Library link; omitted when the controls already sit in the library. */
		inspectHref?: string;
		onchange?: (change: MemoryChange) => void;
	} = $props();

	const isText = $derived(canFlag(path));
	const controls = $derived(flagControls(flags));

	let editing = $state(false);
	let draft = $state("");
	let draftLoading = $state(false);
	let saving = $state(false);
	let editError = $state("");
	/** A non-error state of the editor (an empty start, a settled conflict). */
	let editNotice = $state("");
	let conflict = $state<CorrectionConflict | null>(null);
	let resolving = $state<"current" | "proposed" | null>(null);
	let flagBusy = $state<string | null>(null);
	let confirmingForget = $state(false);
	let forgetting = $state(false);

	// The statement the user sent decides whether a 409 is about it or about
	// an earlier correction the server is still holding for this memory.
	const prompt = $derived(conflict ? conflictPrompt(conflict, draft) : null);

	/**
	 * The editor always starts from the text a correction replaces: the
	 * memory body, or a media memory's bound text. A listing summary or a
	 * receipt excerpt is never a prefill (one is a placeholder, the other is
	 * cut at 240 characters), so a failed read starts empty and says so.
	 */
	async function startEditing() {
		editing = true;
		editError = "";
		editNotice = "";
		conflict = null;
		draft = "";
		draftLoading = true;
		try {
			const raw = await fetchMemoryContent(slug, boundTextPath(path));
			draft = isText ? memoryBody(raw) : mediaBoundText(raw);
		} catch {
			editNotice = isText
				? "The current text could not be loaded. What you save here replaces the whole memory."
				: "This file has no description yet, or it could not be loaded. What you save here becomes its whole description.";
		} finally {
			draftLoading = false;
		}
	}

	function cancelEditing() {
		editing = false;
		conflict = null;
		editError = "";
		editNotice = "";
	}

	async function saveCorrection() {
		if (saving || draftLoading) return;
		if (!draft.trim()) {
			editError = "Write what this memory should say, or forget it instead.";
			return;
		}
		saving = true;
		editError = "";
		editNotice = "";
		try {
			const outcome = correctionOutcome(await correctMemoryFile(slug, path, draft));
			if (outcome.kind === "applied") {
				toast.success("Memory corrected");
				editing = false;
				onchange?.({ kind: "corrected" });
			} else if (outcome.kind === "unchanged") {
				toast.info("The memory already says that.");
				editing = false;
			} else if (outcome.kind === "needs_resolution") {
				conflict = outcome.conflict;
			} else {
				editError = outcome.message;
			}
		} catch (error) {
			editError = error instanceof Error && error.message ? `Could not correct this memory: ${error.message}` : "Could not correct this memory.";
		} finally {
			saving = false;
		}
	}

	async function resolve(keep: "current" | "proposed") {
		if (!conflict || !prompt || resolving) return;
		const pending = prompt.pending;
		resolving = keep;
		try {
			await resolveMemoryCorrection(slug, conflict.conflict_id, keep);
			toast.success(keep === "current" ? "Current statement kept" : pending ? "Earlier correction applied" : "New statement applied");
			conflict = null;
			// A settled earlier conflict leaves the user's own statement unsent:
			// the editor stays open with it so it can be saved now.
			editing = pending;
			editNotice = pending ? "Settled. Your statement below is not applied yet; save it to correct the memory." : "";
			onchange?.({ kind: "resolved", keep });
		} catch {
			editError = "Could not settle the conflict. Both statements are still recorded; try again.";
		} finally {
			resolving = null;
		}
	}

	function flagToast(flag: "pinned" | "exclude_from_proactive", set: boolean) {
		if (flag === "pinned") return set ? "Pinned: recalled on every turn" : "Unpinned";
		return set ? "Excluded from check-ins and reflections" : "Available to check-ins and reflections again";
	}

	async function toggleFlag(control: (typeof controls)[number]) {
		if (flagBusy) return;
		flagBusy = control.flag;
		try {
			const updated = await setMemoryFlags(slug, path, control.next);
			toast.success(flagToast(control.flag, !control.active));
			onchange?.({ kind: "flagged", flags: { pinned: updated.pinned, exclude_from_proactive: updated.exclude_from_proactive } });
		} catch {
			toast.error("Could not update that memory.");
		} finally {
			flagBusy = null;
		}
	}

	async function forget() {
		if (forgetting) return;
		forgetting = true;
		try {
			await deleteMemoryFile(slug, path);
			toast.success("Forgotten");
			confirmingForget = false;
			onchange?.({ kind: "forgotten" });
		} catch {
			toast.error("Could not delete that memory.");
		} finally {
			forgetting = false;
		}
	}
</script>

<div class="controls">
	{#if editing}
		<form class="editor" onsubmit={(e) => { e.preventDefault(); void saveCorrection(); }}>
			<label>
				<span class="editor-label">{isText ? "What this memory should say" : "What Nolune should remember about this file"}</span>
				<textarea class="nl-input editor-text" bind:value={draft} rows="4" disabled={draftLoading || saving || !!conflict} aria-busy={draftLoading}></textarea>
			</label>
			{#if draftLoading}<p class="hint" role="status">Loading the current text…</p>{/if}
			{#if editNotice}<p class="hint" role="status">{editNotice}</p>{/if}
			{#if editError}<p class="hint error" role="alert">{editError}</p>{/if}
			{#if prompt}
				<div class="conflict" role="group" aria-label={prompt.pending ? "An earlier correction is waiting" : "Conflicting corrections"}>
					<p class="conflict-question">{prompt.question}</p>
					<p class="hint">{prompt.note}</p>
					<div class="conflict-options">
						{#each prompt.options as option (option.keep)}
							<div class="conflict-option">
								<p class="conflict-title">{option.title}<span class="hint"> · {recalledWhen(option.corrected_at) || option.corrected_at}</span></p>
								<blockquote class="conflict-statement">{option.statement}</blockquote>
								<button type="button" class={option.keep === "proposed" ? "nl-button" : "nl-button-secondary"} disabled={!!resolving} onclick={() => resolve(option.keep)}>
									{resolving === option.keep ? "Saving…" : option.title}
								</button>
							</div>
						{/each}
					</div>
				</div>
			{/if}
			<div class="row">
				<button type="button" class="nl-button-secondary" disabled={saving || !!resolving} onclick={cancelEditing}>Cancel</button>
				{#if !conflict}
					<button type="submit" class="nl-button" disabled={saving || draftLoading}>{saving ? "Saving…" : "Save correction"}</button>
				{/if}
			</div>
		</form>
	{:else if confirmingForget}
		<div class="forget" role="group" aria-label="Forget this memory">
			<p class="hint">Forget this memory permanently? Its file and search entries are removed, and receipts that cite it will say so.</p>
			<div class="row">
				<button type="button" class="nl-button-secondary" disabled={forgetting} onclick={() => (confirmingForget = false)}>Keep it</button>
				<button type="button" class="nl-button nl-button-destructive" disabled={forgetting} onclick={forget}>{forgetting ? "Forgetting…" : "Forget"}</button>
			</div>
		</div>
	{:else}
		<div class="row">
			{#if inspectHref}
				<a class="nl-button-secondary control" href={inspectHref}><ArrowUpRight size={16} aria-hidden="true" />Inspect</a>
			{/if}
			<button type="button" class="nl-button-secondary control" onclick={startEditing}><Pencil size={16} aria-hidden="true" />Correct</button>
			{#if isText}
				{#each controls as control (control.flag)}
					<button type="button" class="nl-button-secondary control" class:active={control.active} disabled={flagBusy !== null || flags === null} aria-pressed={control.active} onclick={() => toggleFlag(control)}>
						{#if control.flag === "pinned"}
							{#if control.active}<PinOff size={16} aria-hidden="true" />{:else}<Pin size={16} aria-hidden="true" />{/if}
						{:else if control.active}<Eye size={16} aria-hidden="true" />{:else}<EyeOff size={16} aria-hidden="true" />{/if}
						{flagBusy === control.flag ? "Saving…" : control.label}
					</button>
				{/each}
			{/if}
			<button type="button" class="nl-button-secondary control forget-btn" onclick={() => (confirmingForget = true)}><Trash2 size={16} aria-hidden="true" />Forget</button>
		</div>
	{/if}
</div>

<style>
	.controls { display: grid; gap: 8px; min-width: 0; }
	.row { display: flex; flex-wrap: wrap; gap: 8px; }
	.control { padding: 8px 12px; font-size: 13px; }
	.control.active { border-color: var(--primary); }
	.forget-btn { color: var(--destructive); }
	.editor { display: grid; gap: 8px; }
	.editor-label { display: block; font: 500 13px/1.5 var(--font-body); color: var(--text-secondary); margin-bottom: 4px; }
	.editor-text { resize: vertical; min-height: 96px; font-size: 15px; line-height: 1.6; }
	.hint { font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); margin: 0; }
	.error { color: var(--destructive); }
	.conflict { display: grid; gap: 8px; padding: 12px; border: 1px solid var(--primary); border-radius: 12px; background: var(--card); }
	.conflict-question { font: 500 14px/1.5 var(--font-body); color: var(--foreground); margin: 0; }
	.conflict-options { display: grid; gap: 12px; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); }
	.conflict-option { display: grid; gap: 8px; align-content: start; }
	.conflict-title { font: 500 13px/1.5 var(--font-body); color: var(--foreground); margin: 0; }
	.conflict-statement { margin: 0; padding: 8px 12px; border-left: 2px solid var(--border); font: 400 14px/1.6 var(--font-body); color: var(--foreground); white-space: pre-wrap; overflow-wrap: anywhere; }
	.forget { display: grid; gap: 8px; }
</style>
