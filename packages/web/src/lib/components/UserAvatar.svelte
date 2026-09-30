<script lang="ts">
	import { cn } from '$lib/utils';

	interface Props {
		name: string;
		/** Their profile picture's address, if they have one. */
		picture?: string | null;
		class?: string;
	}

	let { name, picture, class: className }: Props = $props();

	/** A stable color per person, so family members are easy to tell apart. */
	const hue = $derived([...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7));
	const initial = $derived(name.trim().charAt(0).toUpperCase() || '?');

	/** A picture that didn't load (gone since the page loaded): the initial stands in. */
	let failed = $state<string | null>(null);
	const shown = $derived(picture && picture !== failed ? picture : null);
</script>

<span
	class={cn(
		'inline-flex size-7 shrink-0 items-center justify-center overflow-hidden rounded-full text-xs font-semibold text-white select-none',
		shown && 'bg-muted',
		className
	)}
	style:background-color={shown ? undefined : `hsl(${hue} 55% 45%)`}
	aria-hidden="true"
>
	{#if shown}
		<img
			src={shown}
			alt=""
			draggable="false"
			decoding="async"
			class="size-full object-cover"
			onerror={() => (failed = shown)}
		/>
	{:else}
		{initial}
	{/if}
</span>
