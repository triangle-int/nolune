<script lang="ts" module>
	/** What the assistant is doing, as its avatar shows it. */
	export type Mood = 'idle' | 'thinking' | 'working' | 'waiting' | 'blocked' | 'done';
</script>

<script lang="ts">
	import { GLYPHS, eyePath, type Avatar, type Shape } from '@btw/core/avatars';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { cn } from '$lib/utils';

	interface Props {
		avatar: Avatar;
		/** Without a mood it holds still, like the avatars on older replies. */
		mood?: Mood;
		/** Width and height in pixels. */
		size?: number;
		/** Shown on hover and read out; without one the picture is hidden from screen readers. */
		label?: string;
		class?: string;
	}

	let { avatar, mood, size = 24, label, class: className }: Props = $props();

	const id = $props.id();
	const glyph = $derived(GLYPHS[avatar]);
	const eyes = $derived(glyph.eyes.map(eyePath));

	/** Holes are knocked out with a mask: the shape's cut, and the eyes unless they sit in a hole. */
	function holed(shape: Shape): boolean {
		return !!shape.cut || (!!shape.eyes && !glyph.solidEyes);
	}
</script>

{#snippet eyeShapes(fill?: string)}
	<g class="eyes" {fill}>
		{#each eyes as d, i (i)}
			<path class="eye" {d} />
		{/each}
	</g>
{/snippet}

{#snippet picture(props: Record<string, unknown> = {})}
	<svg
		{...props}
		viewBox="0 0 24 24"
		width={size}
		height={size}
		fill="currentColor"
		class={cn('avatar shrink-0', className)}
		style:--tone="var(--avatar-{avatar})"
		data-avatar={avatar}
		data-mood={mood}
		role={label ? 'img' : undefined}
		aria-label={label}
		aria-hidden={label ? undefined : 'true'}
	>
		<defs>
			{#each glyph.shapes as shape, i (i)}
				{#if holed(shape)}
					<!-- Larger than the grid, so moving parts keep their holes. -->
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
{/snippet}

{#if label}
	<Tooltip.Root>
		<Tooltip.Trigger>
			{#snippet child({ props })}
				{@render picture(props)}
			{/snippet}
		</Tooltip.Trigger>
		<Tooltip.Content class="max-w-xs">{label}</Tooltip.Content>
	</Tooltip.Root>
{:else}
	{@render picture()}
{/if}

<style>
	/*
	 * Moods. Lengths are grid units (the glyph is 24 wide). Each mood has a pose that holds still
	 * and, on top of it, motion that reduced-motion settings leave out.
	 */
	svg {
		color: var(--tone);
		overflow: visible;
	}
	.body,
	.eyes,
	.eye,
	path {
		transform-box: fill-box;
		transform-origin: center;
	}
	.eyes {
		transition: transform 0.4s ease;
	}

	/* idle, and every mood but blocked: an occasional blink */
	[data-mood] .eye {
		animation: blink 5s infinite;
	}

	/* thinking: eyes up, looking around, a gentle bob */
	[data-mood='thinking'] .eyes {
		transform: translateY(-0.9px);
		animation: ponder 4s ease-in-out infinite;
	}
	[data-mood='thinking'] .body {
		animation: bob 2.6s ease-in-out infinite;
	}

	/* working: eyes on the task, a busy hop, and each avatar's own motion */
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

	/* waiting: a slow pulse, glancing aside at what's queued */
	[data-mood='waiting'] .eyes {
		transform: translateX(0.7px);
	}
	[data-mood='waiting'] .body {
		animation: breathe 2.8s ease-in-out infinite;
	}

	/* blocked: drooping eyes, slumped, muted */
	[data-mood='blocked'] {
		color: color-mix(in oklab, var(--tone) 45%, var(--muted-foreground));
	}
	[data-mood='blocked'] .eye {
		transform-origin: 50% 100%;
		transform: scaleY(0.45);
		animation: none;
	}
	[data-mood='blocked'] .body {
		transform: translateY(0.6px);
	}

	/* done: a happy squash, then idle */
	[data-mood='done'] .body {
		transform-origin: 50% 100%;
		animation: squash 0.7s ease-out;
	}

	@media (prefers-reduced-motion: reduce) {
		svg * {
			animation: none !important;
		}
		[data-mood='waiting'] .body {
			opacity: 0.65;
		}
		[data-mood='working'] .flame {
			transform: scale(1.04, 1.1);
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
	@keyframes ponder {
		0%,
		100% {
			transform: translate(-0.4px, -0.9px);
		}
		50% {
			transform: translate(0.4px, -0.9px);
		}
	}
	@keyframes bob {
		50% {
			transform: translateY(-0.7px);
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
	@keyframes breathe {
		50% {
			transform: scale(0.92);
			opacity: 0.6;
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
