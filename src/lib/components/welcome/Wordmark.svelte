<script lang="ts" module>
	import logo from '$lib/assets/logo.svg?raw';

	/** The letters, from the logo itself, so the two never drift apart. */
	const LETTERS = logo.match(/<path d="([^"]+)"/)?.[1] ?? '';
	/** The three trailing dots, like someone typing: as the logo draws them. */
	export const DOTS = [...logo.matchAll(/<circle ([^>]*)\/>/g)].map(([, attrs]) => {
		const value = (name: string) =>
			Number(attrs.match(new RegExp(`${name}="([\\d.]+)"`))?.[1] ?? 1);
		return { cx: value('cx'), cy: value('cy'), r: value('r'), opacity: value('fill-opacity') };
	});
	/** The logo's width and height in its own units, from its viewBox. */
	export const [WIDTH, HEIGHT] = (
		logo.match(/viewBox="0 0 ([\d.]+) ([\d.]+)"/)?.slice(1) ?? []
	).map(Number);
</script>

<script lang="ts">
	import { cn } from '$lib/utils';

	interface Props {
		/** The letters, for the intro to reveal. */
		letters?: HTMLElement;
		/** Each dot's group, for the intro to move. */
		dots?: SVGGElement[];
		class?: string;
	}

	let { letters = $bindable(), dots = $bindable([]), class: className }: Props = $props();
	const id = $props.id();
</script>

<div
	class={cn('relative', className)}
	style:aspect-ratio="{WIDTH} / {HEIGHT}"
	role="img"
	aria-label="nolune"
>
	<div bind:this={letters} class="absolute inset-0">
		<svg viewBox="0 0 {WIDTH} {HEIGHT}" class="size-full" fill="currentColor" aria-hidden="true">
			<path d={LETTERS} />
		</svg>
	</div>
	<svg
		viewBox="0 0 {WIDTH} {HEIGHT}"
		class="absolute inset-0 size-full overflow-visible"
		fill="currentColor"
		aria-hidden="true"
	>
		<defs>
			<radialGradient id="{id}-glow">
				<stop offset="0" stop-color="currentColor" stop-opacity="0.55" />
				<stop offset="1" stop-color="currentColor" stop-opacity="0" />
			</radialGradient>
		</defs>
		{#each DOTS as dot, i (i)}
			<g bind:this={dots[i]} class="dot">
				{#if i === 0}
					<!-- The first dot is the spark the intro starts from. -->
					<circle cx={dot.cx} cy={dot.cy} r={dot.r * 3.2} fill="url(#{id}-glow)" class="glow" />
				{/if}
				<circle cx={dot.cx} cy={dot.cy} r={dot.r} fill-opacity={dot.opacity} />
			</g>
		{/each}
	</svg>
</div>

<style>
	.dot {
		transform-box: fill-box;
		transform-origin: center;
	}
	.glow {
		opacity: 0;
	}
</style>
