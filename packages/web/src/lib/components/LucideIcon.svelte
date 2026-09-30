<script lang="ts">
	import type { IconNode } from 'lucide';

	interface Props {
		/** A Lucide icon's drawing, as `/api/icons/<name>` serves it. */
		node: IconNode;
		class?: string;
	}

	let { node, class: className = 'size-4' }: Props = $props();
</script>

<svg
	xmlns="http://www.w3.org/2000/svg"
	width="24"
	height="24"
	viewBox="0 0 24 24"
	fill="none"
	stroke="currentColor"
	stroke-width="2"
	stroke-linecap="round"
	stroke-linejoin="round"
	class={className}
	aria-hidden="true"
>
	{#each node as [tag, attrs], i (i)}
		<svelte:element this={tag} {...attrs} />
	{/each}
</svg>
