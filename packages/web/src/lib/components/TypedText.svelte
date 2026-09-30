<script lang="ts">
	import { untrack } from 'svelte';
	import { prefersReducedMotion } from 'svelte/motion';

	/** Shows `text`, typing it in character by character whenever it changes after the first render. */
	let { text }: { text: string } = $props();

	let shown = $state(untrack(() => text));

	$effect(() => {
		const target = text;
		if (untrack(() => shown) === target) return;
		if (prefersReducedMotion.current) {
			shown = target;
			return;
		}
		const chars = Array.from(target);
		// About 20ms a character, so short and long titles both take roughly half a second.
		const duration = Math.min(800, Math.max(250, chars.length * 20));
		const start = performance.now();
		shown = '';
		let frame = requestAnimationFrame(function tick(now) {
			const count = Math.min(chars.length, Math.ceil(((now - start) / duration) * chars.length));
			shown = chars.slice(0, count).join('');
			if (count < chars.length) frame = requestAnimationFrame(tick);
		});
		return () => cancelAnimationFrame(frame);
	});
</script>

<!-- Screen readers get the whole text once, not every partial step. -->
<span class="sr-only">{text}</span><span aria-hidden="true">{shown}</span>
