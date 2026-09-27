<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A translated sentence with markup in it: `text` has `{slots}`, and each slot is a prop of the
	 * same name, a snippet (a link, a name in bold) or plain text. Only `text` is read for slots,
	 * so names and titles people typed are shown as they are.
	 */
	let { text, ...slots }: { text: string; [slot: string]: Snippet | string | undefined } = $props();

	/** The text split at its slots: slot names are at odd indexes. */
	const pieces = $derived(
		text.split(/\{(\w+)\}/).map((piece, i) => (i % 2 === 0 ? piece : (slots[piece] ?? '')))
	);
</script>

<!-- On one line: whitespace between the tags would show between the words. -->
{#each pieces as piece, i (i)}{#if typeof piece === 'string'}{piece}{:else}{@render piece()}{/if}{/each}
