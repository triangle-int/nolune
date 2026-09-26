<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		/** The composer and whatever goes with it (a note under it, a form around it). */
		children: Snippet;
		/** Its height, fade included: the scroll area above pads its end by this much. */
		height?: number;
	}

	let { children, height = $bindable(160) }: Props = $props();
</script>

<!--
	Docks the composer at the bottom of a `relative` scroll area. What scrolls under it fades and
	blurs into it instead of stopping at an edge. Only the composer takes clicks and wheel events;
	the faded strip above it passes them to the page underneath.
-->
<div
	bind:clientHeight={height}
	class="pointer-events-none absolute inset-x-0 bottom-0 px-3 pt-12 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-4"
>
	<div
		aria-hidden="true"
		class="absolute inset-0 bg-background/80 [mask-image:linear-gradient(to_bottom,transparent,black_55%)] backdrop-blur-md"
	></div>
	<div class="pointer-events-auto relative mx-auto max-w-3xl">
		{@render children()}
	</div>
</div>
