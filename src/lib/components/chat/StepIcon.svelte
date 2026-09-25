<script lang="ts" module>
	import { SvelteMap } from 'svelte/reactivity';
	import type { IconNode } from 'lucide';

	/** Fetched icons (null: no such icon), shared by every step on the page. */
	const icons = new SvelteMap<string, IconNode | null>();
	const requested: Record<string, true> = {};

	function request(name: string) {
		if (requested[name]) return;
		requested[name] = true;
		fetch(`/api/icons/${encodeURIComponent(name)}`)
			.then((res) => (res.ok ? (res.json() as Promise<IconNode>) : null))
			.catch(() => null)
			.then((node) => icons.set(name, node));
	}
</script>

<script lang="ts">
	import TerminalIcon from '@lucide/svelte/icons/terminal';

	interface Props {
		/** A Lucide icon name picked by the model, like `cloud-sun`. */
		name: string | null | undefined;
		class?: string;
	}

	let { name, class: className = 'size-4' }: Props = $props();

	const key = $derived(name?.trim().toLowerCase() ?? '');
	/** Undefined while loading. */
	const node = $derived(key ? icons.get(key) : null);

	$effect(() => {
		if (key && !icons.has(key)) request(key);
	});
</script>

{#if node}
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
{:else if node === null}
	<TerminalIcon class={className} />
{:else}
	<!-- Loading: keep the space so the row doesn't shift. -->
	<span class={className}></span>
{/if}
