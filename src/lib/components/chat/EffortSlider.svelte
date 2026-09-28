<!--
	The reasoning level as a trip away from a dying star, after Outer Wilds. The profile's avatar is
	the thumb, and the further out it flies, the harder btw thinks: the star warms, swells into a red
	giant and, at the top level, goes supernova, while the stars stream past faster and the fill
	glows in the star's color. The window is space whatever the theme. See DESIGN.md, "Web UI".
-->
<script lang="ts" module>
	/**
	 * The star's life, one stage per level: its color (the dark theme's avatar colors, as in the
	 * welcome), size, glow (blur and spread) and the halo it lights the window with. The last stage
	 * is the supernova's core, whose fill takes the shock waves' blue.
	 */
	const STAR = [
		{ color: 'var(--avatar-moon)', fill: 'var(--avatar-moon)', size: 14, glow: [10, 1], halo: 80 },
		{
			color: 'var(--avatar-lantern)',
			fill: 'var(--avatar-lantern)',
			size: 20,
			glow: [14, 2],
			halo: 100
		},
		{
			color: 'var(--avatar-campfire)',
			fill: 'var(--avatar-campfire)',
			size: 30,
			glow: [20, 4],
			halo: 130
		},
		{
			color: 'var(--avatar-satellite)',
			fill: 'var(--avatar-satellite)',
			size: 58,
			glow: [30, 8],
			halo: 180
		},
		{ color: '#dff3ff', fill: 'var(--avatar-comet)', size: 12, glow: [26, 10], halo: 240 }
	];
	const HALO_ALPHA = [0.2, 0.24, 0.3, 0.4, 0.55];

	/** The thumb, by stage: its trails' lengths, how fast it bobs, and how much it glows. */
	const TRAILS = [
		[0, 0],
		[12, 0],
		[22, 14],
		[36, 28],
		[60, 48]
	];
	const BOB = [2.8, 2.4, 1.8, 1.2, 0.7];
	const AVATAR_GLOW = [0, 1, 2, 4, 7];
	const DISC_GLOW = [0, 4, 8, 12, 18];
	/** The star lights the thumb from behind once it's big enough. */
	const RIM = [0, 0, 4, 7, 10];

	/**
	 * Stars streaming past, one layer per stage, faster and longer the further out. Each layer's
	 * stars are drawn twice across a strip twice the window's width, which slides by half, so it
	 * loops without a seam. Seeded, so they sit in the same places every time.
	 */
	const LAYERS = [
		{ duration: 90, length: 2, thickness: 2 },
		{ duration: 40, length: 2.5, thickness: 2 },
		{ duration: 16, length: 5, thickness: 1.5 },
		{ duration: 6, length: 14, thickness: 1.5 },
		{ duration: 1.8, length: 34, thickness: 1.5 }
	].map((speed, layer) => {
		let seed = 11 + layer * 97;
		const random = () => (seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296;
		const stars = Array.from({ length: 16 }, () => ({
			x: random() * 50,
			y: 4 + random() * 66,
			alpha: 0.35 + random() * 0.6
		}));
		return {
			...speed,
			streak: layer >= 2,
			stars: [...stars, ...stars.map((star) => ({ ...star, x: star.x + 50 }))]
		};
	});

	/** The supernova's shock waves, and the debris it throws out: where each piece ends up. */
	const SHOCKS = ['var(--avatar-comet)', 'var(--avatar-planet)', 'var(--avatar-moon)'];
	const DEBRIS = [
		{ x: 190, y: -60, duration: 1.5, delay: 0 },
		{ x: 205, y: -25, duration: 1.3, delay: 0.4 },
		{ x: 200, y: 12, duration: 1.7, delay: 0.9 },
		{ x: 185, y: 48, duration: 1.4, delay: 0.2 },
		{ x: 150, y: -90, duration: 1.6, delay: 1.1 },
		{ x: 160, y: 80, duration: 1.5, delay: 0.6 },
		{ x: 110, y: -110, duration: 1.8, delay: 1.4 },
		{ x: 120, y: 105, duration: 1.6, delay: 0.7 }
	];

	/** How long keyboard steps wait for the next one before the level is taken. */
	const SETTLE = 600;
</script>

<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { prefersReducedMotion } from 'svelte/motion';
	import { fade, fly } from 'svelte/transition';
	import type { Avatar } from '@btw/core/avatars';
	import AssistantAvatar, { type Mood } from '$lib/components/AssistantAvatar.svelte';
	import { FIRST_STOP, LAST_INSET, TRACK_START, levelAt, stageOf } from '$lib/effort';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		/** The levels, lowest first. */
		efforts: string[];
		value: string;
		/** A new level, once it's let go of (or keyboard steps settle). */
		onchange: (value: string) => void;
		/** The profile's assistant, which flies the trip. */
		avatar: Avatar;
		class?: string;
	}

	let { efforts, value, onchange, avatar, class: className }: Props = $props();

	const id = $props.id();
	const { m } = getI18n();
	const info: Record<string, { label: string; hint: string } | undefined> = m.model.efforts;

	const last = $derived(Math.max(0, efforts.length - 1));
	const current = $derived(Math.max(0, efforts.indexOf(value)));

	/**
	 * A level picked here that `value` hasn't taken up yet: keyboard steps waiting to settle, or a
	 * change the chat asks about first. It lapses when `value` changes.
	 */
	let pending = $state<{ level: number; over: string; sent: boolean } | null>(null);
	/** Where the pointer holds the thumb while dragging, in levels, fractional between stops. */
	let held = $state<number | null>(null);

	const level = $derived(
		held !== null ? Math.round(held) : pending?.over === value ? pending.level : current
	);
	/** How far along the thumb is, from 0 to 1. */
	const along = $derived((held ?? level) / Math.max(1, last));
	const stage = $derived(stageOf(level, last));
	const star = $derived(STAR[stage]);
	const supernova = $derived(last > 0 && level === last);
	const mood = $derived<Mood>(supernova ? 'working' : stage >= 2 ? 'thinking' : 'idle');
	const labelOf = (i: number) => info[efforts[i]]?.label ?? efforts[i];

	/** Positions along the run of stops, as CSS: `share` from 0 (first stop) to 1 (last). */
	const RUN = `(100% - ${FIRST_STOP + LAST_INSET}px)`;
	const onRun = (share: number) => `calc(${FIRST_STOP}px + ${share} * ${RUN})`;
	/** The fill runs from the track's start to the thumb. */
	const fillWidth = $derived(`calc(${FIRST_STOP - TRACK_START}px + ${along} * ${RUN})`);

	const sunShadow = $derived(
		`0 0 ${star.glow[0]}px ${star.glow[1]}px var(--star), ` +
			(supernova
				? '0 0 60px 18px color-mix(in oklab, var(--avatar-comet) 35%, transparent)'
				: '0 0 0 0 transparent')
	);
	// The rim light is soft and pulled in by its spread, so no hard copy of the ring shows.
	const discShadow = $derived(
		`0 0 ${DISC_GLOW[stage]}px color-mix(in oklab, var(--tone) 50%, transparent), ` +
			(RIM[stage]
				? `-5px 0 ${RIM[stage] * 2}px -3px color-mix(in oklab, var(--star) 60%, transparent)`
				: '0 0 0 0 transparent')
	);

	// Which way the level last moved, and from where: the label rolls that way, the pips light in
	// turn behind the thumb, and arriving at the top level from below sets the star off.
	let seen = untrack(() => level);
	let from = $state(untrack(() => level));
	let direction = $state(1);
	let flashes = $state(0);
	$effect.pre(() => {
		const now = level;
		if (now === seen) return;
		direction = now > seen ? 1 : -1;
		from = seen;
		seen = now;
		if (supernova && direction > 0) untrack(() => flashes++);
	});

	function pipDelay(k: number): number {
		if (held !== null) return 0;
		if (level > from && k > from && k <= level) return (k - from) * 90;
		if (level < from && k > level && k <= from) return (from - k) * 60;
		return 0;
	}

	let input = $state<HTMLInputElement>();
	let lander = $state<HTMLElement>();
	let settle: ReturnType<typeof setTimeout> | undefined;

	function choose(next: number, later = false) {
		pending = { level: next, over: value, sent: false };
		clearTimeout(settle);
		if (later) settle = setTimeout(commit, SETTLE);
		else commit();
	}

	function commit() {
		clearTimeout(settle);
		if (!pending || pending.sent || pending.over !== value) return;
		pending.sent = true;
		const next = efforts[pending.level];
		if (next && next !== value) onchange(next);
	}

	/** A happy squash as the thumb lands on a level. */
	function land() {
		if (prefersReducedMotion.current) return;
		lander?.animate(
			[
				{ transform: 'scale(1)' },
				{ transform: 'scale(1.16, 0.84)', offset: 0.3 },
				{ transform: 'scale(0.94, 1.06)', offset: 0.65 },
				{ transform: 'scale(1)' }
			],
			{ duration: 450, easing: 'ease-out' }
		);
	}

	// Dragging: the thumb leans into fast moves and straightens out once the pointer rests.
	let tilt = $state(0);
	/** The window as a drag starts: its left edge on screen, its width, and CSS pixels per screen pixel. */
	let bounds: { left: number; width: number; scale: number } | null = null;
	let lastMove: { at: number; time: number } | null = null;
	let straighten: ReturnType<typeof setTimeout> | undefined;

	function hold(clientX: number) {
		if (!bounds) return;
		const at = levelAt((clientX - bounds.left) * bounds.scale, bounds.width, last);
		const time = performance.now();
		if (lastMove) {
			const speed = (at - lastMove.at) / Math.max(8, time - lastMove.time);
			tilt = Math.max(-20, Math.min(20, speed * 900));
		}
		lastMove = { at, time };
		clearTimeout(straighten);
		straighten = setTimeout(() => (tilt = 0), 110);
		held = at;
	}

	function onpointerdown(event: PointerEvent & { currentTarget: HTMLElement }) {
		if (event.button !== 0 || last === 0) return;
		const el = event.currentTarget;
		const rect = el.getBoundingClientRect();
		const width = el.offsetWidth || rect.width;
		bounds = { left: rect.left, width, scale: width / rect.width };
		el.setPointerCapture(event.pointerId);
		// So the arrow keys carry on from here. Without preventDefault the press would move focus on
		// to the menu itself (and start selecting text).
		event.preventDefault();
		input?.focus({ preventScroll: true });
		lastMove = null;
		hold(event.clientX);
	}

	function release() {
		if (held === null) return;
		const next = Math.round(held);
		held = null;
		tilt = 0;
		bounds = null;
		lastMove = null;
		clearTimeout(straighten);
		land();
		choose(next);
	}

	const STEPS: Record<string, number> = {
		ArrowRight: 1,
		ArrowUp: 1,
		PageUp: 1,
		ArrowLeft: -1,
		ArrowDown: -1,
		PageDown: -1
	};

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			commit();
			return;
		}
		let next: number;
		if (event.key in STEPS) next = level + STEPS[event.key];
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = last;
		else return;
		// Handled here: the menu would otherwise move focus to its items.
		event.preventDefault();
		next = Math.max(0, Math.min(last, next));
		if (next === level) return;
		land();
		choose(next, true);
	}

	onDestroy(() => {
		// Keyboard steps still settling are taken as the menu closes.
		commit();
		clearTimeout(straighten);
	});

	const reduced = $derived(prefersReducedMotion.current);
</script>

<div class={cn('flex flex-col gap-2', className)}>
	<!-- The range input carries the keyboard and screen readers; the window is for the pointer. -->
	<div
		class="stage dark"
		class:held={held !== null}
		role="presentation"
		style:--star={star.color}
		style:--fill={star.fill}
		style:--tone="var(--avatar-{avatar})"
		{onpointerdown}
		onpointermove={(event) => {
			if (held !== null) hold(event.clientX);
		}}
		onpointerup={release}
		onpointercancel={release}
		onlostpointercapture={release}
	>
		<input
			bind:this={input}
			class="range"
			type="range"
			min="0"
			max={last}
			step="1"
			value={level}
			aria-label={m.model.reasoning}
			aria-valuetext={labelOf(level)}
			aria-describedby="{id}-hint"
			{onkeydown}
			oninput={(event) => choose(Number(event.currentTarget.value), true)}
			onblur={commit}
		/>
		<span class="frame" aria-hidden="true"></span>

		<span
			class="nebula"
			style="left: 110px; top: -50px; width: 180px; height: 120px; background: #2a3a8a"
		></span>
		<span
			class="nebula"
			style="left: 150px; top: 80px; width: 160px; height: 110px; background: #5a2a7a; opacity: 0.3"
		></span>
		{#each LAYERS as layer, i (i)}
			<div class="layer" class:shown={i === stage} aria-hidden="true">
				<div class="strip" style:animation-duration="{layer.duration}s">
					{#each layer.stars as dot, k (k)}
						<span
							class="dot"
							class:streak={layer.streak}
							style:left="{dot.x}%"
							style:top="{dot.y}%"
							style:width="{layer.length}px"
							style:height="{layer.thickness}px"
							style:opacity={dot.alpha}
						></span>
					{/each}
				</div>
			</div>
		{/each}

		<span
			class="halo"
			style:width="{star.halo}px"
			style:height="{star.halo}px"
			style:opacity={HALO_ALPHA[stage]}
			aria-hidden="true"
		></span>
		{#if supernova}
			{#each SHOCKS as color, i (i)}
				<span
					class="shock"
					style:border-color={color}
					style:animation-delay="{i * 0.87}s"
					aria-hidden="true"
				></span>
			{/each}
			{#each DEBRIS as piece, i (i)}
				<span
					class="debris"
					style:--dx="{piece.x}px"
					style:--dy="{piece.y}px"
					style:animation-duration="{piece.duration}s"
					style:animation-delay="{piece.delay}s"
					aria-hidden="true"
				></span>
			{/each}
		{/if}

		<span class="track" aria-hidden="true"></span>
		<span
			class="fill"
			style:width={fillWidth}
			style:box-shadow="0 0 {4 + stage * 3}px color-mix(in oklab, var(--fill) 60%, transparent)"
			aria-hidden="true"
		>
			{#if supernova}<span class="shine"></span>{/if}
		</span>
		{#each efforts as effort, k (effort)}
			<span
				class="pip"
				class:lit={k / Math.max(1, last) <= along + 0.001}
				style:left={onRun(k / Math.max(1, last))}
				style:transition-delay="{pipDelay(k)}ms"
				aria-hidden="true"
			></span>
		{/each}
		<span
			class="sun"
			class:giant={stage === 3}
			class:pulsar={supernova}
			style:width="{star.size}px"
			style:height="{star.size}px"
			style:box-shadow={sunShadow}
			aria-hidden="true"
		></span>

		<div class="thumb" style:left={onRun(along)} aria-hidden="true">
			<span class="trail" style:top="-10px" style:width="{TRAILS[stage][0]}px"></span>
			<span class="trail" style:top="8px" style:width="{TRAILS[stage][1]}px"></span>
			<span class="disc" style:box-shadow={discShadow}></span>
			<div class="tilt" style:transform="rotate({tilt}deg)">
				<div
					class="bob"
					style:animation-duration="{BOB[stage]}s"
					style:filter="drop-shadow(0 0 {AVATAR_GLOW[stage]}px var(--tone))"
				>
					<div class:shake={supernova}>
						<div bind:this={lander} class="lander">
							<AssistantAvatar {avatar} {mood} size={22} />
						</div>
					</div>
				</div>
			</div>
		</div>

		<div class="label" aria-hidden="true">
			{#key level}
				<span
					in:fly={{ y: 10 * direction, duration: reduced ? 0 : 300 }}
					out:fly={{ y: -10 * direction, duration: reduced ? 0 : 300 }}
				>
					{labelOf(level)}
				</span>
			{/key}
		</div>
		{#key flashes}
			{#if flashes}<span class="flash" aria-hidden="true"></span>{/if}
		{/key}
	</div>

	<p id="{id}-hint" class="grid px-2 pb-1 text-xs text-muted-foreground">
		{#key level}
			<span
				class="[grid-area:1/1]"
				in:fade={{ delay: reduced ? 0 : 120, duration: reduced ? 0 : 250 }}
				out:fade={{ duration: reduced ? 0 : 150 }}
			>
				{info[efforts[level]]?.hint ?? ''}
			</span>
		{/key}
	</p>
</div>

<style>
	/*
	 * Pixel positions follow src/lib/effort.ts: the star sits 22px in, the track starts at 46px,
	 * and the star, track and thumb share a line 82px down.
	 */
	.stage {
		position: relative;
		height: 150px;
		border-radius: 18px;
		overflow: hidden;
		isolation: isolate;
		/* The welcome's deep space. */
		background: #04060e;
		cursor: pointer;
		touch-action: pan-y;
		user-select: none;
		-webkit-user-select: none;
		-webkit-tap-highlight-color: transparent;
	}
	.range {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		margin: 0;
		opacity: 0;
		pointer-events: none;
	}
	.frame {
		position: absolute;
		inset: 0;
		z-index: 9;
		border-radius: inherit;
		box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.07);
		pointer-events: none;
	}
	.range:focus-visible ~ .frame {
		box-shadow: inset 0 0 0 2px var(--avatar-comet);
	}

	.nebula {
		position: absolute;
		border-radius: 50%;
		filter: blur(26px);
		opacity: 0.4;
		pointer-events: none;
	}
	.layer {
		position: absolute;
		inset: 0;
		opacity: 0;
		transition: opacity 0.7s ease;
	}
	.layer.shown {
		opacity: 1;
	}
	.strip {
		position: absolute;
		inset: 0 auto 0 0;
		width: 200%;
		animation: stream linear infinite;
	}
	.dot {
		position: absolute;
		border-radius: 2px;
		background: #fff;
	}
	.dot.streak {
		background: linear-gradient(90deg, #fff, rgb(255 255 255 / 0));
	}

	/* The star, at the left, and everything it throws at the thumb. */
	.halo,
	.sun,
	.shock,
	.debris {
		position: absolute;
		left: 22px;
		top: 82px;
		border-radius: 50%;
	}
	.halo {
		z-index: 1;
		background-color: var(--star);
		filter: blur(28px);
		translate: -50% -50%;
		transition:
			width 0.8s cubic-bezier(0.34, 1.3, 0.64, 1),
			height 0.8s cubic-bezier(0.34, 1.3, 0.64, 1),
			opacity 0.6s ease,
			background-color 0.6s ease;
	}
	.sun {
		z-index: 3;
		translate: -50% -50%;
		background-color: var(--star);
		background-image: radial-gradient(circle at 62% 36%, rgb(255 255 255 / 0.6), transparent 62%);
		transition:
			width 0.75s cubic-bezier(0.34, 1.5, 0.64, 1),
			height 0.75s cubic-bezier(0.34, 1.5, 0.64, 1),
			background-color 0.5s ease,
			box-shadow 0.6s ease;
	}
	.sun.giant {
		animation: swell 2.6s ease-in-out infinite;
	}
	.sun.pulsar {
		animation: pulsar 0.9s ease-in-out infinite;
	}
	.shock {
		z-index: 2;
		width: 20px;
		height: 20px;
		margin: -10px 0 0 -10px;
		border: 2px solid;
		opacity: 0;
		pointer-events: none;
		animation: shock 2.6s cubic-bezier(0.2, 0.6, 0.35, 1) infinite;
	}
	.debris {
		z-index: 2;
		width: 3px;
		height: 3px;
		margin: -1.5px 0 0 -1.5px;
		background: #dff3ff;
		box-shadow: 0 0 4px var(--avatar-comet);
		opacity: 0;
		animation: debris cubic-bezier(0.2, 0.7, 0.4, 1) infinite;
	}
	.flash {
		position: absolute;
		inset: 0;
		z-index: 8;
		opacity: 0;
		pointer-events: none;
		background: radial-gradient(
			circle at 9% 55%,
			#fff 0%,
			#dff3ff 18%,
			color-mix(in oklab, var(--avatar-comet) 35%, transparent) 50%,
			transparent 82%
		);
		animation: flash 0.9s ease-out;
	}

	/* The track, out of the star; the fill springs to a level when it's let go. */
	.track,
	.fill {
		position: absolute;
		left: 46px;
		top: 79px;
		z-index: 2;
		height: 6px;
		border-radius: 3px;
	}
	.track {
		right: 12px;
		background: rgb(255 255 255 / 0.1);
	}
	.fill {
		overflow: hidden;
		background-color: var(--fill);
		transition:
			width 0.6s cubic-bezier(0.34, 1.35, 0.64, 1),
			background-color 0.4s ease,
			box-shadow 0.4s ease;
	}
	.shine {
		position: absolute;
		inset: 0;
		background: linear-gradient(
			100deg,
			transparent 30%,
			rgb(255 255 255 / 0.55) 50%,
			transparent 70%
		);
		background-size: 250% 100%;
		animation: shine 2.4s ease-in-out infinite;
	}
	.pip {
		position: absolute;
		top: 82px;
		z-index: 2;
		width: 4px;
		height: 4px;
		margin: -2px 0 0 -2px;
		border-radius: 50%;
		background: rgb(255 255 255 / 0.3);
		transition:
			background-color 0.25s ease,
			scale 0.45s cubic-bezier(0.34, 1.8, 0.64, 1);
	}
	.pip.lit {
		background: rgb(255 255 255 / 0.85);
		scale: 1.25;
	}

	/* The thumb: the avatar in a ring, with trails behind it. */
	.thumb {
		position: absolute;
		top: 82px;
		z-index: 5;
		width: 0;
		height: 0;
		color: var(--tone);
		transition: left 0.6s cubic-bezier(0.34, 1.35, 0.64, 1);
	}
	.held .thumb {
		transition-duration: 0.09s;
		transition-timing-function: cubic-bezier(0.2, 0.8, 0.2, 1);
	}
	.held .fill {
		transition-duration: 0.09s, 0.4s, 0.4s;
		transition-timing-function: cubic-bezier(0.2, 0.8, 0.2, 1), ease, ease;
	}
	.trail {
		position: absolute;
		right: 14px;
		height: 2px;
		border-radius: 1px;
		background: linear-gradient(270deg, currentColor, transparent);
		opacity: 0.75;
		transition: width 0.55s cubic-bezier(0.2, 0.8, 0.2, 1);
	}
	.disc {
		position: absolute;
		left: -17px;
		top: -17px;
		width: 34px;
		height: 34px;
		box-sizing: border-box;
		border: 1.5px solid currentColor;
		border-radius: 50%;
		background: #04060e;
		transition:
			box-shadow 0.5s ease,
			scale 0.45s cubic-bezier(0.34, 1.8, 0.64, 1);
	}
	.held .disc {
		scale: 1.1;
	}
	.tilt {
		position: absolute;
		left: -11px;
		top: -11px;
		transition: transform 0.25s cubic-bezier(0.2, 0.8, 0.2, 1);
	}
	.bob {
		animation: bob 2.4s ease-in-out infinite;
		transition: filter 0.5s ease;
	}
	.shake {
		animation: shake 0.14s steps(2) infinite;
	}
	.lander {
		display: flex;
		transform-origin: 50% 90%;
	}

	.label {
		position: absolute;
		left: 14px;
		top: 11px;
		z-index: 6;
		display: grid;
		font-size: 14px;
		font-weight: 600;
		color: #fff;
		text-shadow: 0 1px 8px rgb(0 0 0 / 0.6);
		white-space: nowrap;
	}
	.label > span {
		grid-area: 1 / 1;
	}

	@keyframes stream {
		to {
			transform: translateX(-50%);
		}
	}
	@keyframes swell {
		50% {
			scale: 1.07;
		}
	}
	@keyframes pulsar {
		12% {
			scale: 1.35;
		}
		30% {
			scale: 0.95;
		}
	}
	@keyframes shock {
		0% {
			transform: scale(0.3);
			opacity: 0.95;
		}
		85% {
			opacity: 0.25;
		}
		100% {
			transform: scale(18);
			opacity: 0;
		}
	}
	@keyframes debris {
		0% {
			transform: none;
			opacity: 1;
		}
		100% {
			transform: translate(var(--dx), var(--dy)) scale(0.4);
			opacity: 0;
		}
	}
	@keyframes flash {
		12% {
			opacity: 0.95;
		}
	}
	@keyframes shine {
		0% {
			background-position: 100% 0;
		}
		55%,
		100% {
			background-position: 0% 0;
		}
	}
	@keyframes bob {
		50% {
			transform: translateY(-1.5px);
		}
	}
	@keyframes shake {
		25% {
			transform: translate(-1px, 0.5px);
		}
		50% {
			transform: translate(1px, -0.5px);
		}
		75% {
			transform: translate(-0.5px, -1px);
		}
	}

	/* The level and the star's stage still show; nothing moves. */
	@media (prefers-reduced-motion: reduce) {
		.stage,
		.stage * {
			animation: none !important;
			transition: none !important;
		}
	}
</style>
