<script lang="ts">
	/**
	 * The accessible companion status (#86): one sentence from the reducer
	 * that names what Nolune is doing, with the related computer, conversation,
	 * Activity run or handoff card linked when the companion's `slug` is
	 * known. A polite live region, so a screen reader hears each change
	 * without the moon's motion; visible wherever the caller places it.
	 */
	import { companionStatusSegments, type CompanionState } from "$lib/companion/state.js";

	let {
		state,
		name = "",
		slug = null,
		class: className = "",
	}: { state: CompanionState; name?: string; slug?: string | null; class?: string } = $props();

	const segments = $derived(companionStatusSegments(state, name || undefined, slug));
</script>

<p class="companion-status {className}" role="status" aria-live="polite" data-kind={state.kind}>
	{#each segments as segment, i (i)}{#if "href" in segment}<a class="companion-status-link" href={segment.href}>{segment.text}</a>{:else}{segment.text}{/if}{/each}
</p>

<style>
	.companion-status {
		margin: 0;
		font: 400 13px/1.5 var(--font-body);
		color: var(--text-secondary);
	}
	.companion-status-link {
		color: var(--primary);
		text-decoration: underline;
		text-underline-offset: 2px;
		pointer-events: auto;
	}
	.companion-status-link:hover {
		color: var(--foreground);
	}
</style>
