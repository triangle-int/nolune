<!--
	A new profile's welcome, outside the profile's sidebar layout. Everything is a dot: the wordmark's
	three trailing dots become the eight avatar colors, one of them becomes the assistant, imported
	memories fly into the Memory page's dot grid, and the grid gathers into the avatar as the first
	chat opens. See DESIGN.md, "Welcome".
-->
<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { fade, fly } from 'svelte/transition';
	import { prefersReducedMotion } from 'svelte/motion';
	import { enhance } from '$app/forms';
	import { goto, onNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import Volume2Icon from '@lucide/svelte/icons/volume-2';
	import VolumeXIcon from '@lucide/svelte/icons/volume-x';
	import { isAvatar, type Avatar } from '@btw/core/avatars';
	import type { ExportedFact } from '@btw/core/memory-export';
	import type { ImportedNote } from '@btw/core';
	import AssistantAvatar from '$lib/components/AssistantAvatar.svelte';
	import AvatarPicker from '$lib/components/AvatarPicker.svelte';
	import { Button } from '$lib/components/ui/button';
	import IntroSky, { type SkyStage } from '$lib/components/welcome/IntroSky.svelte';
	import MemoryArrival from '$lib/components/welcome/MemoryArrival.svelte';
	import MemoryStep from '$lib/components/welcome/MemoryStep.svelte';
	import ModelStep from '$lib/components/welcome/ModelStep.svelte';
	import Wordmark, { DOTS, WIDTH } from '$lib/components/welcome/Wordmark.svelte';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';
	import { avatarTint, tintStyle } from '$lib/tint';
	import { cn } from '$lib/utils';
	import { UNDER, duck, music, play, quiet, setSoundsOn, wake } from '$lib/welcome/sounds';

	let { data } = $props();
	const { m } = getI18n();
	const prefs = getPreferences();

	type Step = 'model' | 'avatar' | 'memory';
	/** Fixed for the visit: adding a model doesn't take the step away while it's on screen. */
	const steps: Step[] = untrack(() => [
		...(data.needsModel ? (['model'] as const) : []),
		'avatar',
		'memory'
	]);

	let phase = $state<'intro' | 'welcome' | 'steps' | 'hello' | 'arrival'>('intro');
	let step = $state(0);
	let back = $state(false);

	let sky = $state<SkyStage>('dark');
	let skipped = $state(false);
	/** Deep space behind the intro, dark whatever the theme; the page takes over at the welcome. */
	let space = $state(true);
	let origins = $state<{ x: number; y: number }[]>([]);
	let skyCenter = $state<{ x: number; y: number } | null>(null);

	let picked = $state<Avatar>(untrack(() => data.welcome.avatar));
	/** The avatar whose tint the page has taken on, once it's chosen. */
	let tinted = $state<Avatar | null>(null);
	const tint = $derived(tinted && avatarTint(tinted));
	let savingAvatar = $state(false);
	let avatarProblem = $state<string | null>(null);
	let stageEl = $state<HTMLElement>();

	let imported = $state<{ notes: ImportedNote[]; lines: ExportedFact[] } | null>(null);

	let wordmarkBox = $state<HTMLElement>();
	/** Hidden until the intro's animations hold it, so the page doesn't flash the whole logo first. */
	let drawing = $state(false);
	let letters = $state<HTMLElement>();
	let dots = $state<SVGGElement[]>([]);

	const reduced = $derived(prefersReducedMotion.current);
	const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
	const centerOf = (el: Element) => {
		const r = el.getBoundingClientRect();
		return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
	};

	$effect(() => setSoundsOn(prefs.sounds));

	/** Bumped to stop a running intro. */
	let introRun = 0;
	/** The song has been started; it plays on, quieter, under the questions. */
	let songOn = false;
	/** Off to the chat: the song, if it's playing its end, isn't cut off on the way. */
	let finishing = false;

	/**
	 * Space, to a song: stars come out, one of them draws the wordmark, its three dots type, then
	 * fly out as the eight avatar colors and orbit it like planets, pooling into a glow behind the
	 * welcome as the song lifts. About 24 seconds; a click skips it.
	 */
	async function intro() {
		const run = ++introRun;
		const alive = () => run === introRun && phase === 'intro';
		await tick();
		if (!wordmarkBox || !letters || dots.length < 3) return;

		// Timed to the song: it rises out of silence, goes quiet for the drawing, comes back for
		// the planets, and lifts at 23.6 seconds, when the welcome comes up.
		songOn = true;
		await music(0);
		if (!alive()) return;
		sky = 'stars';
		await wait(9000);
		if (!alive()) return;

		const box = wordmarkBox.getBoundingClientRect();
		const unit = box.width / WIDTH;
		const [first, second, third] = dots;
		const spark = `${(innerWidth / 2 - box.left) / unit - DOTS[0].cx}px, ${(innerHeight / 2 - box.top) / unit - DOTS[0].cy}px`;
		const start = `${-DOTS[0].cx + 40}px, 0px`;
		const draw = 2800;
		const sweep = 0.5;

		// A star brightens and draws the letters.
		first.animate(
			[
				{ transform: `translate(${spark}) scale(0)`, easing: 'cubic-bezier(.2,.8,.2,1)' },
				{ transform: `translate(${spark}) scale(1.35)`, offset: 0.22, easing: 'ease-in-out' },
				{
					transform: `translate(${spark}) scale(1)`,
					offset: 0.36,
					easing: 'cubic-bezier(.6,0,.4,1)'
				},
				{
					transform: `translate(${start}) scale(1)`,
					offset: sweep,
					easing: 'cubic-bezier(.45,0,.25,1)'
				},
				{ transform: 'translate(0px, 0px) scale(1)' }
			],
			{ duration: draw, fill: 'backwards' }
		);
		first
			.querySelector('.glow')
			?.animate(
				[
					{ opacity: 0 },
					{ opacity: 1, offset: 0.22 },
					{ opacity: 0.7, offset: 0.9 },
					{ opacity: 0 }
				],
				{ duration: draw }
			);
		// The letters appear behind the light as it sweeps across them.
		letters.animate(
			[
				{ clipPath: 'inset(0 100% 0 0)' },
				{ clipPath: 'inset(0 100% 0 0)', offset: sweep, easing: 'cubic-bezier(.45,0,.25,1)' },
				{ clipPath: `inset(0 ${((WIDTH - DOTS[0].cx) / WIDTH) * 100}% 0 0)` }
			],
			{ duration: draw, fill: 'backwards' }
		);
		for (const dot of [second, third]) {
			dot.animate([{ opacity: 0 }, { opacity: 0 }], { duration: draw, fill: 'backwards' });
		}
		drawing = true;
		await wait(draw);
		if (!alive()) return;

		// Typing.
		const beat = 620;
		const gap = 180;
		dots.forEach((dot, k) => {
			dot.animate(
				[
					{ transform: 'translateY(0)' },
					{ transform: 'translateY(-55px)', offset: 0.3 },
					{ transform: 'translateY(0)', offset: 0.6 },
					{ transform: 'translateY(0)' }
				],
				{ duration: beat, delay: k * gap, iterations: 2, easing: 'ease-in-out' }
			);
		});
		await wait(2 * beat + 2 * gap);
		if (!alive()) return;

		// The dots break loose as eight colors, and circle the wordmark like planets.
		origins = dots.map(centerOf);
		// Around the letters: the dots are gone.
		skyCenter = centerOf(letters);
		sky = 'orbit';
		for (const dot of dots) {
			dot.animate([{ opacity: 1 }, { opacity: 0, transform: 'scale(1.8)' }], {
				duration: 250,
				fill: 'forwards'
			});
		}
		await wait(7000);
		if (!alive()) return;

		// They pool into a glow; the song goes quiet for a breath.
		sky = 'aurora';
		wordmarkBox.animate(
			[{ opacity: 1 }, { opacity: 0, transform: 'translateY(-16px) scale(.97)' }],
			{ duration: 1400, easing: 'ease-in', fill: 'forwards' }
		);
		await wait(2400);
		if (!alive()) return;
		// Space gives way to the page as the song lifts.
		space = false;
		await wait(800);
		if (!alive()) return;
		phase = 'welcome';
		// From here the welcome waits for "Let's go"; the song rings on a moment, then goes quiet.
		await wait(2500);
		if (phase === 'welcome') duck(UNDER, 4);
	}

	function skipIntro() {
		if (phase !== 'intro') return;
		introRun++;
		if (songOn) duck(UNDER, 1.2);
		else music(0, { level: UNDER });
		songOn = true;
		skipped = true;
		space = false;
		sky = 'aurora';
		phase = 'welcome';
	}

	function begin() {
		wake();
		duck(UNDER, 2.5);
		phase = 'steps';
	}

	function nextStep() {
		back = false;
		if (step + 1 < steps.length) step++;
		else finish();
	}

	/** The avatar's color washes over the page from where it stands, and it says hello. */
	async function hello() {
		const origin = stageEl ? centerOf(stageEl) : { x: innerWidth / 2, y: innerHeight / 2 };
		const root = document.documentElement;
		root.style.setProperty('--wash-x', `${origin.x}px`);
		root.style.setProperty('--wash-y', `${origin.y}px`);
		play('wash');
		const apply = async () => {
			tinted = picked;
			phase = 'hello';
			sky = 'gone';
			await tick();
		};
		if (document.startViewTransition && !reduced) {
			root.classList.add('btw-wash');
			await document.startViewTransition(apply).finished.catch(() => {});
			root.classList.remove('btw-wash');
		} else {
			await apply();
		}
		await wait(reduced ? 900 : 1700);
		quiet(1.2, 'wash');
		phase = 'steps';
		nextStep();
	}

	/**
	 * On to the profile's first chat. After the memories arrive, the song plays its end over it and
	 * the page gives way slowly; otherwise the song fades.
	 */
	async function finish() {
		finishing = true;
		if (phase !== 'arrival') quiet(3, 'music');
		else if ('startViewTransition' in document && !reduced) {
			document.documentElement.classList.add('btw-arrive');
		}
		await goto(resolve('/p/[slug]', { slug: data.welcome.slug }), { replaceState: true });
	}

	// The avatar glides from here to its place on the new-chat page.
	onNavigate((navigation) => {
		if (!document.startViewTransition || reduced) return;
		return new Promise((done) => {
			const transition = document.startViewTransition(async () => {
				done();
				await navigation.complete;
			});
			transition.finished.finally(() => document.documentElement.classList.remove('btw-arrive'));
		});
	});

	onMount(() => {
		// Loads the sounds; after the click on Create, the intro can play them.
		wake();
		if (reduced) skipIntro();
		else intro();
		return () => {
			if (!finishing) quiet(0.5);
		};
	});
</script>

<svelte:head>
	{#if tint}
		<!-- Colors worked out from layout.css, not from anything people type. -->
		<!-- eslint-disable-next-line svelte/no-at-html-tags -->
		{@html tintStyle(tint)}
	{/if}
</svelte:head>

<svelte:window
	onkeydown={(event) => event.key === 'Escape' && skipIntro()}
	onpointerdown={() => wake()}
/>

<IntroSky
	stage={sky}
	{space}
	{origins}
	center={skyCenter}
	still={skipped || reduced}
	class={cn('transition-opacity duration-700', phase === 'steps' && 'opacity-50')}
/>

<div class="relative flex h-dvh flex-col bg-transparent">
	<header class="relative z-20 flex h-14 shrink-0 items-center justify-end gap-2 px-4">
		{#if phase === 'steps' || phase === 'hello' || phase === 'arrival'}
			<ol
				class="absolute left-1/2 flex -translate-x-1/2 gap-1.5"
				aria-label={m.welcome.progress(Math.min(step + 1, steps.length), steps.length)}
				in:fade
			>
				{#each steps as s, i (s)}
					<li
						class="h-1 w-8 rounded-full bg-foreground/15 transition-colors duration-500"
						style:background-color={i <= step || phase !== 'steps'
							? tinted
								? `var(--avatar-${tinted})`
								: 'var(--foreground)'
							: undefined}
					></li>
				{/each}
			</ol>
		{/if}
		<Button
			variant="ghost"
			size="icon-sm"
			class={cn(
				'text-muted-foreground transition-colors duration-700',
				space && 'text-white/50 hover:bg-white/10 hover:text-white dark:hover:bg-white/10'
			)}
			aria-label={prefs.sounds ? m.welcome.soundsOff : m.welcome.soundsOn}
			title={prefs.sounds ? m.welcome.soundsOff : m.welcome.soundsOn}
			onclick={() => prefs.set({ sounds: !prefs.sounds })}
		>
			{#if prefs.sounds}<Volume2Icon />{:else}<VolumeXIcon />{/if}
		</Button>
	</header>

	<main
		class="flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-4 pb-[max(2rem,env(safe-area-inset-bottom))]"
	>
		<div class="my-auto flex w-full flex-col items-center py-6">
			{#if phase === 'intro'}
				<!-- A click anywhere skips it. -->
				<button
					type="button"
					class="fixed inset-0 z-10 cursor-default"
					aria-label={m.welcome.skipIntro}
					onclick={skipIntro}
				></button>
				<div
					bind:this={wordmarkBox}
					class="w-[min(22rem,70vw)] text-white"
					class:invisible={!drawing}
				>
					<Wordmark bind:letters bind:dots />
				</div>
			{:else if phase === 'welcome'}
				<div class="flex max-w-xl flex-col items-center gap-5 text-center">
					<h1
						class="text-4xl font-semibold tracking-tight text-balance sm:text-5xl"
						in:fly={{ y: 14, duration: reduced ? 0 : 600 }}
					>
						{m.welcome.title}
					</h1>
					<p
						class="text-lg text-muted-foreground"
						in:fly={{ y: 14, delay: 80, duration: reduced ? 0 : 600 }}
					>
						{m.welcome.subtitle}
					</p>
					<div
						class="flex flex-col items-center gap-3 pt-3"
						in:fly={{ y: 14, delay: 160, duration: reduced ? 0 : 600 }}
					>
						<Button size="lg" class="h-12 gap-2 px-8 text-base" onclick={begin}>
							{m.welcome.go}
							<ArrowRightIcon />
						</Button>
						<span class="text-xs text-muted-foreground">{m.welcome.takesAMinute}</span>
					</div>
				</div>
			{:else if phase === 'steps'}
				{#key step}
					<div
						class="w-full max-w-2xl"
						in:fly={{ y: back ? -16 : 16, duration: reduced ? 0 : 400, delay: reduced ? 0 : 150 }}
						out:fade={{ duration: reduced ? 0 : 150 }}
					>
						{#if steps[step] === 'model'}
							<ModelStep isAdmin={data.isAdmin} keys={data.keys} ondone={nextStep} />
						{:else if steps[step] === 'avatar'}
							<form
								method="POST"
								action="?/avatar"
								class="space-y-6"
								use:enhance={({ submitter, cancel, formData }) => {
									// A tile only shows its avatar; "This one" saves it.
									const value = submitter?.getAttribute('value');
									if (submitter?.getAttribute('name') === 'avatar') {
										cancel();
										if (isAvatar(value)) picked = value;
										return;
									}
									formData.set('avatar', picked);
									savingAvatar = true;
									avatarProblem = null;
									return async ({ result }) => {
										savingAvatar = false;
										if (result.type === 'success') hello();
										else if (result.type === 'failure') {
											avatarProblem = String(result.data?.avatarError ?? '');
										}
									};
								}}
							>
								<div class="space-y-3 text-center">
									<h2 class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
										{m.welcome.avatar.title}
									</h2>
									<p class="text-muted-foreground">
										{m.welcome.avatar.subtitle(data.welcome.name)}
									</p>
								</div>
								<AvatarPicker avatar={picked} hero bind:stage={stageEl} />
								{#if avatarProblem}
									<p class="text-center text-sm text-destructive" role="alert">{avatarProblem}</p>
								{/if}
								<div class="flex justify-center">
									<Button
										type="submit"
										name="confirm"
										size="lg"
										class="h-11 min-w-44 px-8"
										disabled={savingAvatar}
									>
										{m.welcome.avatar.thisOne}
									</Button>
								</div>
							</form>
						{:else if steps[step] === 'memory'}
							<MemoryStep
								prompt={data.exportPrompt}
								avatar={picked}
								onimported={(result, lines) => {
									if (!result.added) finish();
									else {
										imported = { notes: result.notes, lines };
										phase = 'arrival';
									}
								}}
								onskip={finish}
							/>
						{/if}
					</div>
				{/key}
			{:else if phase === 'hello'}
				<div class="relative flex flex-col items-center gap-8">
					<span
						class="wash-ring pointer-events-none absolute top-[60px] left-1/2 size-32 rounded-full border-2"
						style:border-color="var(--avatar-{picked})"
						aria-hidden="true"
					></span>
					<div class="bounce">
						<AssistantAvatar avatar={picked} mood="done" size={120} />
					</div>
					<p
						class="text-3xl font-semibold tracking-tight sm:text-4xl"
						in:fly={{ y: 14, delay: 250, duration: reduced ? 0 : 500 }}
					>
						{m.welcome.avatar.hello}
					</p>
				</div>
			{:else if phase === 'arrival' && imported}
				<div class="w-full max-w-2xl">
					<MemoryArrival
						avatar={picked}
						notes={imported.notes}
						lines={imported.lines}
						ondone={finish}
					/>
				</div>
			{/if}
		</div>
	</main>

	{#if phase === 'intro'}
		<p
			class="pointer-events-none fixed inset-x-0 bottom-6 text-center text-xs text-white/40"
			in:fade={{ delay: 2000, duration: 1200 }}
			out:fade={{ duration: 300 }}
		>
			{m.welcome.skipHint}
		</p>
	{/if}
</div>

<style>
	/* The chosen avatar's tint washes in as a circle growing from where it stands. */
	:global {
		html.btw-wash::view-transition-old(root) {
			animation: none;
		}
		html.btw-wash::view-transition-new(root) {
			animation: btw-wash 0.75s cubic-bezier(0.4, 0, 0.2, 1);
		}
		/* Into the chat after the memories: slowly, the avatar gliding to its place. */
		html.btw-arrive::view-transition-old(root) {
			animation: btw-leave 1.2s ease-in both;
		}
		html.btw-arrive::view-transition-new(root) {
			animation: btw-enter 1.4s ease-out 0.7s both;
		}
		html.btw-arrive::view-transition-group(btw-assistant),
		html.btw-arrive::view-transition-old(btw-assistant),
		html.btw-arrive::view-transition-new(btw-assistant) {
			animation-duration: 2s;
			animation-timing-function: cubic-bezier(0.45, 0, 0.2, 1);
		}
		@keyframes btw-leave {
			to {
				opacity: 0;
			}
		}
		@keyframes btw-enter {
			from {
				opacity: 0;
			}
		}
		@keyframes btw-wash {
			from {
				clip-path: circle(0 at var(--wash-x) var(--wash-y));
			}
			to {
				clip-path: circle(150vmax at var(--wash-x) var(--wash-y));
			}
		}
	}

	.bounce {
		animation: bounce 0.9s cubic-bezier(0.34, 1.56, 0.64, 1);
	}
	@keyframes bounce {
		0% {
			transform: translateY(40px) scale(0.6, 1.3);
		}
		45% {
			transform: translateY(-12px) scale(1.15, 0.85);
		}
		70% {
			transform: translateY(0) scale(0.95, 1.05);
		}
	}
	.wash-ring {
		opacity: 0;
		animation: wash-ring 1.4s ease-out 0.15s;
	}
	@keyframes wash-ring {
		from {
			opacity: 0.8;
			transform: translate(-50%, -50%) scale(0.6);
		}
		to {
			opacity: 0;
			transform: translate(-50%, -50%) scale(6);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.bounce,
		.wash-ring {
			animation: none;
		}
	}
</style>
