<script lang="ts" module>
	/** The moods the page shows: a blink now and then, busy, and a happy squash when picked. */
	export type Mood = 'idle' | 'working' | 'done';
</script>

<script lang="ts">
	import { GLYPHS, eyePath, type Avatar, type Shape } from '@nolune/core/avatars';

	interface Props {
		avatar: Avatar;
		mood?: Mood;
		/** Width and height in pixels. */
		size?: number;
		class?: string;
	}

	let { avatar, mood = 'idle', size = 24, class: className }: Props = $props();

	const id = $props.id();
	const glyph = $derived(GLYPHS[avatar]);
	const eyes = $derived(glyph.eyes.map(eyePath));

	function holed(shape: Shape): boolean {
		return !!shape.cut || (!!shape.eyes && !glyph.solidEyes);
	}
</script>

<!-- The app's avatar (src/lib/components/AssistantAvatar.svelte) without its tooltip, and with
     only the moods this page uses. -->

{#snippet eyeShapes(fill?: string)}
	<g class="eyes" {fill}>
		{#each eyes as d, i (i)}
			<path class="eye" {d} />
		{/each}
	</g>
{/snippet}

<svg
	viewBox="0 0 24 24"
	width={size}
	height={size}
	fill="currentColor"
	class={className}
	data-mood={mood}
	aria-hidden="true"
>
	<defs>
		{#each glyph.shapes as shape, i (i)}
			{#if holed(shape)}
				<mask id="{id}-{i}" maskUnits="userSpaceOnUse" x="-4" y="-4" width="32" height="32">
					<rect x="-4" y="-4" width="32" height="32" fill="#fff" />
					{#if shape.cut}
						<path d={shape.cut} fill="#000" />
					{/if}
					{#if shape.eyes && !glyph.solidEyes}
						{@render eyeShapes('#000')}
					{/if}
				</mask>
			{/if}
		{/each}
	</defs>
	<g class="body">
		{#each glyph.shapes as shape, i (i)}
			<path
				d={shape.d}
				class={shape.part}
				style:--i={i}
				mask={holed(shape) ? `url(#${id}-${i})` : undefined}
			/>
		{/each}
		{#if glyph.solidEyes}
			{@render eyeShapes()}
		{/if}
	</g>
</svg>

<style>
	svg {
		flex-shrink: 0;
		overflow: visible;
	}
	.body,
	.eyes,
	.eye,
	path {
		transform-box: fill-box;
		transform-origin: center;
	}

	.eye {
		animation: blink 5s infinite;
	}

	[data-mood='working'] .eyes {
		transform: translateY(0.45px);
	}
	[data-mood='working'] .body {
		animation: hop 0.55s ease-in-out infinite;
	}
	[data-mood='working'] .tip {
		animation: beacon 0.9s steps(1) infinite;
	}
	[data-mood='working'] .flame {
		transform-origin: 50% 100%;
		animation: flare 0.45s ease-in-out infinite alternate;
	}
	[data-mood='working'] .lure {
		animation: glow 0.6s ease-in-out infinite alternate;
	}
	[data-mood='working'] .dash {
		animation: flicker 0.8s steps(1) infinite;
		animation-delay: calc(var(--i) * -0.23s);
	}
	[data-mood='working'] .tail {
		animation: stream 0.5s ease-in-out infinite alternate;
		animation-delay: calc(var(--i) * -0.17s);
	}
	[data-mood='working'] .panel {
		animation: flap 0.7s ease-in-out infinite alternate;
	}

	[data-mood='done'] .body {
		transform-origin: 50% 100%;
		animation: squash 0.7s ease-out;
	}

	@media (prefers-reduced-motion: reduce) {
		svg * {
			animation: none !important;
		}
	}

	@keyframes blink {
		0%,
		94%,
		100% {
			transform: scaleY(1);
		}
		97% {
			transform: scaleY(0.12);
		}
	}
	@keyframes hop {
		50% {
			transform: translateY(-0.9px);
		}
	}
	@keyframes beacon {
		50% {
			opacity: 0.3;
		}
	}
	@keyframes flare {
		to {
			transform: scale(1.06, 1.14);
		}
	}
	@keyframes glow {
		to {
			transform: scale(1.2);
		}
	}
	@keyframes flicker {
		40% {
			opacity: 0.25;
		}
		70% {
			opacity: 0.8;
		}
	}
	@keyframes stream {
		to {
			transform: translate(-0.8px, 0.37px);
		}
	}
	@keyframes flap {
		to {
			transform: scaleY(0.7);
		}
	}
	@keyframes squash {
		30% {
			transform: scale(1.12, 0.86);
		}
		60% {
			transform: scale(0.95, 1.07);
		}
	}
</style>
