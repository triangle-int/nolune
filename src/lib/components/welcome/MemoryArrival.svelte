<script lang="ts">
	import { onMount, tick } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { cubicOut } from 'svelte/easing';
	import { Tween, prefersReducedMotion } from 'svelte/motion';
	import type { Avatar } from '@btw/core/avatars';
	import type { ExportedFact } from '@btw/core/memory-export';
	import type { ImportedNote } from '@btw/core';
	import AssistantAvatar, { type Mood } from '$lib/components/AssistantAvatar.svelte';
	import { factInk, factShade } from '$lib/components/memory/DotGrid.svelte';
	import { getI18n } from '$lib/i18n';
	import { memoryTopic } from '$lib/memory';
	import { play, sparkle } from '$lib/welcome/sounds';

	interface Props {
		avatar: Avatar;
		/** What the import added, note by note. */
		notes: ImportedNote[];
		/** The pasted lines, for the dots to fly out of; empty when the model rewrote them. */
		lines: ExportedFact[];
		/** Everything has landed and gathered into the avatar. */
		ondone: () => void;
	}

	let { avatar, notes, lines, ondone }: Props = $props();
	const { m } = getI18n();

	/*
	 * Every dot lands at full strength, then fades to its fact's age, as the Memory page shades
	 * it: what the other assistant learned years ago settles lighter than last week's.
	 */

	/** Lines shown in the box before they fly; the rest leave from its bottom edge. */
	const VISIBLE_LINES = 12;
	/** Sparkles are capped, so a big import doesn't turn into noise. */
	const MAX_SPARKLES = 40;
	const now = Date.now();

	let phase = $state<'lines' | 'grid' | 'settled' | 'gathering'>('lines');
	let mood = $state<Mood>('thinking');
	let landed = $state(0);

	const rows = $derived.by(() => {
		let offset = 0;
		return notes.map((note) => {
			const row = { path: note.path, title: memoryTopic(note.path), facts: note.facts, offset };
			offset += note.facts.length;
			return row;
		});
	});
	const total = $derived(rows.reduce((sum, row) => sum + row.facts.length, 0));
	const shownLines = $derived(
		(lines.length
			? lines.map((l) => l.text)
			: notes.flatMap((n) => n.facts.map((f) => f.text))
		).slice(0, VISIBLE_LINES)
	);

	const count = new Tween(0, { duration: 400, easing: cubicOut });
	$effect(() => {
		count.set(landed, { duration: prefersReducedMotion.current ? 0 : 400 });
	});

	let avatarEl = $state<HTMLElement>();
	let lineEls = $state<HTMLElement[]>([]);
	let linesBox = $state<HTMLElement>();
	let dotEls = $state<HTMLElement[]>([]);

	/** A memory on its way from its line to its dot, in viewport pixels. */
	interface Flyer {
		from: { x: number; y: number };
		to: { x: number; y: number };
		color: string;
		delay: number;
		/** How high the arc goes, and how far it leans. */
		lift: number;
		swing: number;
		land: () => void;
	}
	let flyers = $state.raw<Flyer[]>([]);

	/** Flies a dot along its arc, and says when it lands. */
	function flight(f: Flyer): Attachment<HTMLElement> {
		return (el) => {
			const animation = el.animate(
				[
					{ transform: `translate(${f.from.x}px, ${f.from.y}px) scale(0.4)`, opacity: 0 },
					{ opacity: 1, offset: 0.12 },
					{
						transform: `translate(${(f.from.x + f.to.x) / 2 + f.swing}px, ${f.lift}px) scale(1)`,
						offset: 0.55
					},
					{ transform: `translate(${f.to.x}px, ${f.to.y}px) scale(1)`, opacity: 1 }
				],
				{ duration: 850, delay: f.delay, easing: 'cubic-bezier(.45,.05,.3,1)', fill: 'both' }
			);
			animation.finished.then(f.land, () => {});
			return () => animation.cancel();
		};
	}

	const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
	const centerOf = (el: Element) => {
		const r = el.getBoundingClientRect();
		return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
	};

	/** The line a fact came from: by its words, else spread over the lines shown. */
	function sourceLine(text: string, i: number): number {
		const found = shownLines.findIndex((line) => text.endsWith(line) || line.endsWith(text));
		return found !== -1 ? found : i % Math.max(1, shownLines.length);
	}

	async function run() {
		if (prefersReducedMotion.current || total === 0) {
			phase = 'settled';
			landed = total;
			mood = 'done';
			await wait(total ? 2200 : 600);
			ondone();
			return;
		}
		await wait(900);

		// Where each line is now, before the box gives way to the grid.
		const sources = shownLines.map((_, i) => (lineEls[i] ? centerOf(lineEls[i]) : null));
		const boxBottom = linesBox ? centerOf(linesBox) : { x: innerWidth / 2, y: innerHeight / 2 };
		const facts = rows.flatMap((row) => row.facts);
		const from = facts.map((fact, i) => sources[sourceLine(fact.text, i)] ?? boxBottom);

		phase = 'grid';
		mood = 'working';
		await tick();

		const stagger = Math.min(45, 1000 / Math.max(1, facts.length));
		const every = Math.max(1, Math.ceil(facts.length / MAX_SPARKLES));
		await new Promise<void>((allLanded) => {
			let left = facts.length;
			flyers = facts.map((fact, i) => {
				const to = dotEls[i] ? centerOf(dotEls[i]) : boxBottom;
				return {
					from: from[i],
					to,
					color: factInk(1),
					delay: i * stagger,
					lift: Math.min(from[i].y, to.y) - 60 - (i % 5) * 12,
					swing: ((i % 7) - 3) * 18,
					land: () => {
						landed = Math.max(landed, i + 1);
						if (i % every === 0) sparkle(i / every, Math.ceil(facts.length / every));
						if (--left === 0) allLanded();
					}
				};
			});
		});
		flyers = [];

		phase = 'settled';
		mood = 'done';
		play('chord');
		await wait(1800);

		// Everything gathers into the avatar, and the chat opens.
		phase = 'gathering';
		mood = 'working';
		play('gather');
		const home = avatarEl ? centerOf(avatarEl) : { x: innerWidth / 2, y: 80 };
		await Promise.all(
			dotEls.map((dot, i) => {
				const at = centerOf(dot);
				return dot
					.animate(
						[
							{ transform: 'translate(0, 0) scale(1)', opacity: 1 },
							{
								transform: `translate(${home.x - at.x}px, ${home.y - at.y}px) scale(0.3)`,
								opacity: 0
							}
						],
						{
							duration: 650,
							delay: Math.min(i * 8, 400),
							easing: 'cubic-bezier(.5,0,.75,0)',
							fill: 'forwards'
						}
					)
					.finished.catch(() => {});
			})
		);
		mood = 'done';
		await wait(350);
		ondone();
	}

	onMount(() => {
		run();
	});
</script>

<div class="flex w-full flex-col items-center gap-8">
	<div bind:this={avatarEl} style:view-transition-name="btw-assistant">
		<AssistantAvatar {avatar} {mood} size={64} />
	</div>

	{#if phase === 'lines'}
		<div
			bind:this={linesBox}
			class="w-full space-y-1 rounded-3xl border bg-card px-5 py-4 font-mono text-[13px] leading-relaxed text-foreground/80"
		>
			{#each shownLines as line, i (i)}
				<p bind:this={lineEls[i]} class="truncate">{line}</p>
			{/each}
		</div>
	{:else}
		<div class="text-center">
			<div class="text-6xl font-semibold tracking-tight tabular-nums">
				{Math.round(count.current)}
			</div>
			<p class="mt-1 text-muted-foreground" aria-live="polite">
				{m.welcome.arrival.thingsIKnow(total)}
			</p>
		</div>
		<!-- While the dots gather, the box and labels fade and only the dots move. -->
		<div
			class="w-full space-y-0.5 rounded-3xl border px-3 py-4 transition-colors duration-500 sm:px-5"
			class:border-transparent={phase === 'gathering'}
		>
			{#each rows as row (row.path)}
				<div class="flex items-start gap-4 rounded-2xl px-2 py-2.5">
					<span
						class="w-24 shrink-0 truncate text-[15px] leading-4 text-foreground/70 transition-opacity duration-500 sm:w-36"
						class:opacity-0={phase === 'gathering'}
					>
						{row.title}
					</span>
					<div class="flex min-h-4 flex-1 flex-wrap gap-2.5">
						{#each row.facts as fact, i (i)}
							<span
								bind:this={dotEls[row.offset + i]}
								aria-hidden="true"
								class="dot size-4 rounded-full"
								style:background-color={factInk(
									phase === 'grid' ? 1 : factShade(fact.learnedAt, now)
								)}
								style:opacity={landed > row.offset + i ? 1 : 0}
							></span>
						{/each}
					</div>
				</div>
			{/each}
		</div>
	{/if}
</div>

<div class="pointer-events-none fixed inset-0 z-50" aria-hidden="true">
	{#each flyers as flyer, i (i)}
		<span
			{@attach flight(flyer)}
			class="absolute top-0 left-0 -mt-1.5 -ml-1.5 size-3 rounded-full"
			style:background-color={flyer.color}
		></span>
	{/each}
</div>

<style>
	.dot {
		transition:
			opacity 150ms ease-out,
			background-color 1.4s ease-in-out 200ms;
	}
</style>
