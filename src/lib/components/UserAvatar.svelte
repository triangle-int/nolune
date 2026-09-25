<script lang="ts">
	import { cn } from '$lib/utils';

	interface Props {
		name: string;
		class?: string;
	}

	let { name, class: className }: Props = $props();

	/** A stable color per person, so family members are easy to tell apart. */
	const hue = $derived([...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7));
	const initial = $derived(name.trim().charAt(0).toUpperCase() || '?');
</script>

<span
	class={cn(
		'inline-flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold text-white select-none',
		className
	)}
	style:background-color="hsl({hue} 55% 45%)"
	aria-hidden="true"
>
	{initial}
</span>
