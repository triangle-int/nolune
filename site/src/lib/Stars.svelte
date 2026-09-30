<script lang="ts">
	import { random } from './random';

	interface Props {
		count?: number;
		seed?: number;
	}

	let { count = 60, seed = 1 }: Props = $props();

	const stars = $derived.by(() => {
		const next = random(seed);
		return Array.from({ length: count }, () => ({
			x: next() * 100,
			y: next() * 100,
			r: next() < 0.85 ? 0.7 : 1.3,
			opacity: 0.25 + next() * 0.55
		}));
	});
</script>

<svg class="stars" aria-hidden="true">
	{#each stars as star, i (i)}
		<circle cx="{star.x}%" cy="{star.y}%" r={star.r} opacity={star.opacity} />
	{/each}
</svg>

<style>
	.stars {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		fill: var(--cream);
		pointer-events: none;
	}
</style>
