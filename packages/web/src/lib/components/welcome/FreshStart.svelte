<!--
	The welcome's end with no memories to bring over. The eight colors from the intro circle the
	assistant once more, like planets, and spiral into it as the chat opens, to the song's last
	phrase. See DESIGN.md, "Welcome".
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { fly } from 'svelte/transition';
	import { prefersReducedMotion } from 'svelte/motion';
	import { AVATARS, type Avatar } from '@nolune/core/avatars';
	import AssistantAvatar, { type Mood } from '$lib/components/AssistantAvatar.svelte';
	import { getI18n } from '$lib/i18n';
	import { PHRASE, lastPhrase } from '$lib/welcome/sounds';

	interface Props {
		avatar: Avatar;
		/** Everything has gathered into the avatar. */
		ondone: () => void;
	}

	let { avatar, ondone }: Props = $props();
	const { m } = getI18n();

	/** The orbits' tilt, as in the intro. */
	const TILT = (-11 * Math.PI) / 180;
	const clamp = (t: number) => Math.min(1, Math.max(0, t));
	const easeOut = (t: number) => 1 - (1 - clamp(t)) ** 3;
	const easeInOut = (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);
	const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

	const reduced = prefersReducedMotion.current;
	let mood = $state<Mood>('idle');
	/** The words are up. */
	let said = $state(reduced);
	/** How many planets have gathered into the avatar, which glows brighter with each. */
	let gathered = $state(0);
	/** How far out the widest orbit reaches, in pixels, once the planets are out. */
	let reach = $state(0);
	/** The orbits' faint rings show while the planets circle. */
	let rings = $state(false);

	const planets: HTMLElement[] = [];
	const planet =
		(i: number): Attachment<HTMLElement> =>
		(el) => {
			planets[i] = el;
		};

	/** Planet `i`'s orbit, as a share of the widest: the first closest in. */
	const orbitOf = (i: number) => 0.5 + (0.5 * i) / (AVATARS.length - 1);

	/** Moves the planets for `t` seconds into the phrase; true once they've all gathered. */
	function place(t: number, reach: number): boolean {
		let home = 0;
		planets.forEach((el, i) => {
			const scale = orbitOf(i);
			// Out from the avatar as the phrase starts; in along a spiral as it turns, one after another.
			const gather = clamp((t - PHRASE.turn - i * 0.2) / 2.4);
			const r = reach * scale * easeOut(t / 2.4) * (1 - easeInOut(gather));
			// Closer in goes faster, as planets do, and faster still as they spiral in.
			const a = i * 2.4 + t * 0.55 * scale ** -1.5 + gather * 2.5;
			const ex = Math.cos(a) * r;
			const ey = Math.sin(a) * r * 0.3;
			const x = ex * Math.cos(TILT) - ey * Math.sin(TILT);
			const y = ex * Math.sin(TILT) + ey * Math.cos(TILT);
			el.style.transform = `translate(${x}px, ${y}px) scale(${(0.85 + 0.15 * Math.sin(a)) * (1 - 0.5 * gather)})`;
			el.style.opacity = String(clamp(t * 1.5) * (1 - clamp((gather - 0.8) / 0.2)));
			// Behind the avatar on the far side of the orbit, in front on the near side.
			el.style.zIndex = Math.sin(a) > 0 ? '20' : '0';
			if (gather >= 1) home++;
		});
		if (home > gathered) gathered = home;
		return home === planets.length;
	}

	onMount(() => {
		let alive = true;
		let frame = 0;
		(async () => {
			// The song goes to its last phrase; everything from here keeps time with it.
			await lastPhrase();
			if (!alive) return;
			if (reduced) {
				mood = 'done';
				await wait(2200);
				if (alive) ondone();
				return;
			}
			const start = performance.now();
			reach = Math.min(210, innerWidth * 0.44);
			rings = true;
			let home = false;
			const tick = (now: number) => {
				if (!alive) return;
				const t = (now - start) / 1000;
				if (!said && t >= PHRASE.second) {
					said = true;
					mood = 'done';
				}
				if (!home && mood === 'done' && t >= PHRASE.turn) {
					mood = 'working';
					rings = false;
				}
				if (!home && place(t, reach)) {
					home = true;
					mood = 'done';
				}
				if (home && t >= PHRASE.open) {
					ondone();
					return;
				}
				frame = requestAnimationFrame(tick);
			};
			frame = requestAnimationFrame(tick);
		})();
		return () => {
			alive = false;
			cancelAnimationFrame(frame);
		};
	});
</script>

<div class="flex flex-col items-center gap-10">
	<div class="relative py-12">
		<div class="bounce relative z-10" style:view-transition-name="nolune-assistant">
			<span
				class="glow pointer-events-none absolute inset-0 -z-10 rounded-full blur-xl"
				style:background-color="var(--avatar-{avatar})"
				style:opacity={(gathered / AVATARS.length) * 0.7}
				style:scale={1 + (gathered / AVATARS.length) * 0.8}
				aria-hidden="true"
			></span>
			<AssistantAvatar {avatar} {mood} size={96} />
		</div>
		{#if reach}
			<!-- The orbits, as in the intro. -->
			<svg
				class="pointer-events-none absolute top-1/2 left-1/2 overflow-visible text-foreground transition-opacity duration-[2000ms]"
				class:opacity-0={!rings}
				width="1"
				height="1"
				aria-hidden="true"
			>
				{#each AVATARS as name, i (name)}
					<ellipse
						rx={reach * orbitOf(i)}
						ry={reach * orbitOf(i) * 0.3}
						transform="rotate(-11)"
						fill="none"
						stroke="currentColor"
						stroke-opacity="0.1"
					/>
				{/each}
			</svg>
		{/if}
		{#if !reduced}
			{#each AVATARS as name, i (name)}
				<span
					{@attach planet(i)}
					class="pointer-events-none absolute top-1/2 left-1/2 rounded-full opacity-0"
					style:width="{8 + (i % 3) * 3}px"
					style:height="{8 + (i % 3) * 3}px"
					style:margin="-{4 + (i % 3) * 1.5}px 0 0 -{4 + (i % 3) * 1.5}px"
					style:background-color="var(--avatar-{name})"
					style:box-shadow="0 0 14px 2px var(--avatar-{name})"
					aria-hidden="true"
				></span>
			{/each}
		{/if}
	</div>

	<div class="flex min-h-20 flex-col items-center gap-2 text-center">
		{#if said}
			<h2
				class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl"
				in:fly={{ y: 14, duration: reduced ? 0 : 700 }}
			>
				{m.welcome.fresh.title}
			</h2>
			<p class="text-muted-foreground" in:fly={{ y: 14, delay: 120, duration: reduced ? 0 : 700 }}>
				{m.welcome.fresh.subtitle}
			</p>
		{/if}
	</div>
</div>

<style>
	.bounce {
		animation: bounce 0.9s cubic-bezier(0.34, 1.56, 0.64, 1);
	}
	@keyframes bounce {
		0% {
			transform: translateY(30px) scale(0.6, 1.3);
		}
		45% {
			transform: translateY(-10px) scale(1.12, 0.88);
		}
		70% {
			transform: translateY(0) scale(0.96, 1.04);
		}
	}
	.glow {
		transition:
			opacity 0.6s ease-out,
			scale 0.6s ease-out;
	}
	@media (prefers-reduced-motion: reduce) {
		.bounce {
			animation: none;
		}
	}
</style>
