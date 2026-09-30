<script lang="ts">
	import { backOut } from 'svelte/easing';
	import { prefersReducedMotion } from 'svelte/motion';
	import { scale } from 'svelte/transition';
	import { AVATARS, type Avatar } from '@nolune/core/avatars';
	import AssistantAvatar from './AssistantAvatar.svelte';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		/** The avatar shown as picked. */
		avatar: Avatar;
		/** Big, for a new profile's welcome: the stage above a single row of the roster. */
		hero?: boolean;
		/** The stage, for whoever wants to know where the pick is shown. */
		stage?: HTMLElement;
		/** Makes the big avatar pokeable: it squashes, and this is called (the welcome's yap). */
		onpoke?: () => void;
	}

	let { avatar, hero = false, stage = $bindable(), onpoke }: Props = $props();

	const { m } = getI18n();

	/** A few stars behind the big avatar: where (percent), how big (px), and when they twinkle. */
	const STARS = [
		[14, 18, 2, 0],
		[80, 12, 3, 1.4],
		[88, 46, 2, 2.6],
		[9, 60, 3, 0.8],
		[24, 86, 2, 2.1],
		[74, 80, 2, 3.2],
		[50, 8, 2, 1.9]
	];

	/** What's under the pointer shows off its motion: a tile's avatar, or the big one. */
	let hovered = $state<Avatar | 'stage' | null>(null);

	function poke(event: MouseEvent & { currentTarget: HTMLElement }) {
		if (!prefersReducedMotion.current) {
			event.currentTarget.animate(
				[
					{ transform: 'scale(1)' },
					{ transform: 'scale(1.14, 0.86)', offset: 0.3 },
					{ transform: 'scale(0.94, 1.06)', offset: 0.65 },
					{ transform: 'scale(1)' }
				],
				{ duration: 380, easing: 'ease-out' }
			);
		}
		onpoke?.();
	}

	function hover(what: Avatar | 'stage') {
		return {
			onpointerenter: () => (hovered = what),
			onpointerleave: () => {
				if (hovered === what) hovered = null;
			}
		};
	}
</script>

<!--
	Laid out like a character select: the pick up close on a starry stage lit in its color, and the roster
	as submit buttons for the form around it. Each tile takes its avatar's color, and hovering one
	shows the avatar's working motion.
-->
<div class={cn('grid', hero ? 'gap-4' : 'gap-2 sm:grid-cols-[9.5rem_1fr]')}>
	<div
		bind:this={stage}
		aria-hidden="true"
		class={cn(
			'stage relative flex flex-col items-center justify-center gap-2 overflow-hidden rounded-3xl border',
			hero ? 'h-56 sm:h-64' : 'h-36 sm:h-auto'
		)}
		style:--stage-tone="var(--avatar-{avatar})"
		{...hover('stage')}
	>
		{#each STARS as [x, y, size, delay], i (i)}
			<span
				class="star absolute rounded-full"
				style:left="{x}%"
				style:top="{y}%"
				style:width="{size}px"
				style:height="{size}px"
				style:animation-delay="-{delay}s"
			></span>
		{/each}
		<!-- A new pick pops in with a happy squash; hovered, it gets busy. -->
		{#key avatar}
			<div
				class="relative"
				in:scale={{
					start: 0.4,
					duration: prefersReducedMotion.current ? 0 : 450,
					easing: backOut
				}}
			>
				{#if onpoke}
					<!-- For the pointer only: the roster below is how it's picked. -->
					<button type="button" tabindex="-1" class="block cursor-pointer" onclick={poke}>
						<AssistantAvatar
							{avatar}
							mood={hovered === 'stage' ? 'working' : 'done'}
							size={hero ? 112 : 72}
						/>
					</button>
				{:else}
					<AssistantAvatar
						{avatar}
						mood={hovered === 'stage' ? 'working' : 'done'}
						size={hero ? 112 : 72}
					/>
				{/if}
			</div>
		{/key}
		<span class="relative text-sm font-medium">{m.avatars[avatar]}</span>
	</div>

	<div class={cn('grid grid-cols-4 gap-2', hero && 'sm:grid-cols-8')}>
		{#each AVATARS as name (name)}
			{@const picked = name === avatar}
			<button
				type="submit"
				name="avatar"
				value={name}
				aria-pressed={picked}
				style:--tone="var(--avatar-{name})"
				{...hover(name)}
				class={cn(
					'group flex flex-col items-center gap-1.5 rounded-2xl border border-transparent px-1 pt-3 pb-2 text-xs text-muted-foreground transition-colors outline-none hover:bg-(--tone)/10 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/30',
					picked &&
						'border-(--tone)/70 bg-(--tone)/15 font-medium text-foreground hover:bg-(--tone)/15'
				)}
			>
				<AssistantAvatar
					avatar={name}
					mood={hovered === name ? 'working' : picked ? 'idle' : undefined}
					size={36}
					class="transition-transform duration-200 motion-safe:group-hover:-translate-y-0.5 motion-safe:group-active:scale-90"
				/>
				<span>{m.avatars[name]}</span>
			</button>
		{/each}
	</div>
</div>

<style>
	/* Registered so the stage's light fades from one avatar's color to the next. */
	@property --stage-tone {
		syntax: '<color>';
		inherits: true;
		initial-value: transparent;
	}

	.stage {
		background: radial-gradient(
			circle at 50% 40%,
			color-mix(in oklab, var(--stage-tone) 26%, transparent),
			transparent 70%
		);
		transition: --stage-tone 0.5s ease;
	}

	.star {
		background: var(--stage-tone);
		opacity: 0.55;
		animation: twinkle 3.6s ease-in-out infinite;
	}
	@keyframes twinkle {
		50% {
			opacity: 0.15;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.star {
			animation: none;
		}
	}
</style>
