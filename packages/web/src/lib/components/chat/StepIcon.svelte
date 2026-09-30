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
	import LucideIcon from '$lib/components/LucideIcon.svelte';

	interface Props {
		/** A Lucide icon name picked by the model, like `cloud-sun`. */
		name: string | null | undefined;
		class?: string;
		/** Shown when there is no name or Lucide has no icon by that name. */
		fallback?: typeof TerminalIcon;
	}

	let { name, class: className = 'size-4', fallback: Fallback = TerminalIcon }: Props = $props();

	const key = $derived(name?.trim().toLowerCase() ?? '');
	/** Undefined while loading. */
	const node = $derived(key ? icons.get(key) : null);

	$effect(() => {
		if (key && !icons.has(key)) request(key);
	});
</script>

{#if node}
	<LucideIcon {node} class={className} />
{:else if node === null}
	<Fallback class={className} />
{:else}
	<!-- Loading: keep the space so the row doesn't shift. -->
	<span class={className}></span>
{/if}
