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
	import { fadeOutMusic, play, playMusic, setSoundsOn, wake } from '$lib/welcome/sounds';

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

	/**
	 * A point of light draws the wordmark, its three dots type, then fly out as the eight avatar
	 * colors and pool into an aurora behind the welcome. About five seconds; a click skips it.
	 */
	async function intro() {
		const run = ++introRun;
		const alive = () => run === introRun && phase === 'intro';
		await tick();
		if (!wordmarkBox || !letters || dots.length < 3) return;
		const box = wordmarkBox.getBoundingClientRect();
		const unit = box.width / WIDTH;
		const [first, second, third] = dots;
		const spark = `${(innerWidth / 2 - box.left) / unit - DOTS[0].cx}px, ${(innerHeight / 2 - box.top) / unit - DOTS[0].cy}px`;
		const start = `${-DOTS[0].cx + 40}px, 0px`;
		const draw = 1900;
		const sweep = 0.55;

		playMusic('intro', 0.05);
		play('shimmer');
		first.animate(
			[
				{ transform: `translate(${spark}) scale(0)`, easing: 'cubic-bezier(.2,.8,.2,1)' },
				{ transform: `translate(${spark}) scale(1.35)`, offset: 0.22, easing: 'ease-in-out' },
				{
					transform: `translate(${spark}) scale(1)`,
					offset: 0.38,
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
				{
					duration: draw
				}
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
		await wait(draw * sweep);
		if (!alive()) return;
		play('trace');
		await wait(draw * (1 - sweep));
		if (!alive()) return;

		// Typing.
		const beat = 480;
		const gap = 140;
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
			for (let i = 0; i < 2; i++) {
				setTimeout(() => alive() && play('tick', { pan: (k - 1) * 0.3 }), k * gap + i * beat);
			}
		});
		await wait(2 * beat + 2 * gap);
		if (!alive()) return;

		// The dots break loose as eight colors.
		origins = dots.map(centerOf);
		skyCenter = centerOf(wordmarkBox);
		sky = 'orbit';
		play('burst');
		for (const dot of dots) {
			dot.animate([{ opacity: 1 }, { opacity: 0, transform: 'scale(1.8)' }], {
				duration: 250,
				fill: 'forwards'
			});
		}
		await wait(1500);
		if (!alive()) return;

		sky = 'aurora';
		play('swell');
		wordmarkBox.animate(
			[{ opacity: 1 }, { opacity: 0, transform: 'translateY(-16px) scale(.97)' }],
			{
				duration: 450,
				fill: 'forwards'
			}
		);
		await wait(400);
		if (!alive()) return;
		// From here the welcome waits for "Let's go".
		fadeOutMusic(2.5);
		phase = 'welcome';
	}

	function skipIntro() {
		if (phase !== 'intro') return;
		introRun++;
		fadeOutMusic(1);
		skipped = true;
		sky = 'aurora';
		phase = 'welcome';
	}

	function begin() {
		wake();
		play('click');
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
		playMusic('hello', 0.25);
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
		fadeOutMusic(1.5);
		phase = 'steps';
		nextStep();
	}

	/** On to the profile's first chat. */
	async function finish() {
		// Carries on for a moment over the new chat, where it's up to them again.
		fadeOutMusic(3);
		await goto(resolve('/p/[slug]', { slug: data.welcome.slug }), { replaceState: true });
	}

	// The avatar glides from here to its place on the new-chat page.
	onNavigate((navigation) => {
		if (!document.startViewTransition || reduced) return;
		return new Promise((done) => {
			document.startViewTransition(async () => {
				done();
				await navigation.complete;
			});
		});
	});

	onMount(() => {
		// Loads the sounds; after the click on Create, the intro can play them.
		wake();
		if (reduced) skipIntro();
		else intro();
		return () => fadeOutMusic(0.5);
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
			class="text-muted-foreground"
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
					class="w-[min(22rem,70vw)] text-foreground"
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
										if (isAvatar(value) && value !== picked) {
											play('click');
											picked = value;
										}
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
										playMusic('arrival', 0.3);
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
			class="pointer-events-none fixed inset-x-0 bottom-6 text-center text-xs text-muted-foreground"
			in:fade={{ delay: 1200 }}
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
