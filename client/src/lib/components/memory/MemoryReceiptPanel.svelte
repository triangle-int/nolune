<script lang="ts">
	/**
	 * "Why did Nolune remember this?" (#84): the receipt of one assistant
	 * message. Each recalled memory shows its canonical source, the excerpt
	 * that was injected, when and why it was retrieved and a confidence
	 * bucket, with controls to inspect, correct, pin, exclude or forget it.
	 * A source that was forgotten since the reply is said so, not hidden.
	 * The receipt is re-read from the server after every change, so what is
	 * shown is always what the companion's own files say.
	 */
	import { untrack } from "svelte";
	import { fetchMemory, fetchMemoryReceipt } from "$lib/api/client.js";
	import type { MemoryFlags, RecalledMemory } from "$lib/api/types.js";
	import { displayName } from "$lib/memory/library.js";
	import { confidenceLabel, flagBadges, isMediaMemory, reasonLabel, recalledWhen, receiptHeading, receiptSummary, sourceStatusCopy, uniqueMemories } from "$lib/memory/receipts.js";
	import MemoryControls, { type MemoryChange } from "./MemoryControls.svelte";
	import ChevronDown from "@lucide/svelte/icons/chevron-down";

	let {
		slug,
		chatId,
		messageId,
		memories,
		companionName = "",
		readonly = false,
	}: {
		slug: string;
		chatId: string;
		messageId: string;
		memories: RecalledMemory[];
		companionName?: string;
		/** Sample rendering (the design-system gallery): open by default, no requests, no controls. */
		readonly?: boolean;
	} = $props();

	let expanded = $state(false);
	const open = $derived(readonly || expanded);
	/** The receipt as re-read after a change; the prop is the initial value. */
	let refreshed = $state<RecalledMemory[] | null>(null);
	let flagsByPath = $state<Map<string, MemoryFlags>>(new Map());
	let flagsKnown = $state(false);
	let refreshError = $state("");

	// One row per memory: a long memory is cited once per vector chunk that
	// ranked, and the re-read receipt comes straight from the server.
	const shown = $derived(uniqueMemories(refreshed ?? memories));
	const heading = $derived(receiptHeading(companionName));

	// A new receipt from the chat (a reload) replaces whatever this panel re-read.
	$effect(() => {
		void memories;
		untrack(() => {
			refreshed = null;
		});
	});

	async function loadFlags() {
		try {
			const entries = await fetchMemory(slug);
			flagsByPath = new Map(entries.map((entry) => [entry.path, { pinned: entry.pinned, exclude_from_proactive: entry.exclude_from_proactive }]));
			flagsKnown = true;
		} catch {
			flagsKnown = false;
		}
	}

	function toggle() {
		expanded = !expanded;
		if (expanded && !flagsKnown && !readonly) void loadFlags();
	}

	async function refresh() {
		refreshError = "";
		try {
			const [receipt] = await Promise.all([fetchMemoryReceipt(slug, chatId, messageId), loadFlags()]);
			if (receipt) refreshed = receipt.memories;
		} catch {
			refreshError = "The change was saved, but this receipt could not be re-read. Reload to see it.";
		}
	}

	function onchange(change: MemoryChange) {
		void change;
		void refresh();
	}

	function flagsFor(memory: RecalledMemory): MemoryFlags | null {
		if (!flagsKnown || isMediaMemory(memory)) return null;
		return flagsByPath.get(memory.path) ?? null;
	}
</script>

{#if shown.length === 0}
	<p class="receipt-none">{receiptSummary(shown)}</p>
{:else}
	<div class="receipt">
		<button type="button" class="receipt-toggle" aria-expanded={open} aria-controls={`receipt-${messageId}`} onclick={toggle}>
			<span class="receipt-heading">{heading}</span>
			<span class="receipt-count">{receiptSummary(shown)}</span>
			<span class="receipt-chevron" class:open aria-hidden="true"><ChevronDown size={16} /></span>
		</button>
		{#if open}
			<ul class="receipt-list" id={`receipt-${messageId}`}>
				{#each shown as memory (memory.path)}
					{@const missing = memory.source_status === "missing"}
					{@const media = isMediaMemory(memory)}
					{@const flags = flagsFor(memory)}
					<li class="receipt-item" class:missing>
						<div class="receipt-source">
							<span class="receipt-name">{media ? memory.path : displayName(memory.path)}</span>
							<span class="receipt-path">{memory.source}</span>
							{#if missing}<span class="badge badge-missing">Forgotten</span>{/if}
							{#if media}<span class="badge">Media</span>{/if}
							{#each flagBadges(flags) as badge (badge)}<span class="badge">{badge}</span>{/each}
						</div>
						{#if memory.excerpt}
							<blockquote class="receipt-excerpt">{memory.excerpt}</blockquote>
						{/if}
						<p class="receipt-meta">
							<span>{reasonLabel(memory)}</span>
							<span>{confidenceLabel(memory.confidence)}</span>
							<span>Recalled {recalledWhen(memory.retrieved_at) || "at an unknown time"}</span>
						</p>
						{#if missing}
							<p class="receipt-status">{sourceStatusCopy(memory)}</p>
						{:else if !readonly}
							<MemoryControls {slug} path={memory.path} {flags} inspectHref={`/${encodeURIComponent(slug)}/memory?open=${encodeURIComponent(memory.path)}`} {onchange} />
						{/if}
					</li>
				{/each}
			</ul>
			{#if refreshError}<p class="receipt-status" role="alert">{refreshError}</p>{/if}
		{/if}
	</div>
{/if}

<style>
	.receipt-none { margin: 4px 0 0; font: 400 12px/1.5 var(--font-body); color: var(--text-muted); }
	.receipt { margin-top: 6px; max-width: 100%; min-width: 0; }
	.receipt-toggle { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; min-height: 44px; padding: 6px 0; background: none; border: 0; color: var(--text-secondary); font: 400 13px/1.5 var(--font-body); cursor: pointer; text-align: left; }
	.receipt-toggle:hover { color: var(--foreground); }
	.receipt-heading { color: var(--primary); font-weight: 500; }
	.receipt-count { color: var(--text-muted); }
	.receipt-chevron { display: inline-flex; transition: transform 160ms ease; }
	.receipt-chevron.open { transform: rotate(180deg); }
	.receipt-list { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
	.receipt-item { display: grid; gap: 8px; padding: 14px 16px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); min-width: 0; }
	.receipt-item.missing { border-style: dashed; }
	.receipt-source { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; min-width: 0; }
	.receipt-name { font: 500 14px/1.5 var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.receipt-path { font: 400 12px/1.5 var(--font-mono); color: var(--text-muted); overflow-wrap: anywhere; }
	.badge { font: 500 11px/1.4 var(--font-body); letter-spacing: 0.04em; text-transform: uppercase; color: var(--primary); border: 1px solid var(--border); border-radius: 999px; padding: 1px 8px; }
	.badge-missing { color: var(--destructive); }
	.receipt-excerpt { margin: 0; padding: 0 0 0 12px; border-left: 2px solid var(--primary); font: 400 14px/1.6 var(--font-body); color: var(--foreground); white-space: pre-wrap; overflow-wrap: anywhere; }
	.receipt-meta { display: flex; flex-wrap: wrap; gap: 4px 12px; margin: 0; font: 400 12px/1.5 var(--font-body); color: var(--text-secondary); }
	.receipt-status { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); }
	@media (prefers-reduced-motion: reduce) { .receipt-chevron { transition: none; } }
</style>
