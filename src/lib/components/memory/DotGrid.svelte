<script lang="ts" module>
	import type { MemoryFact } from '@btw/core';

	export interface Topic {
		/** The note, relative to the memory folder. */
		path: string;
		title: string;
		/** The folder it's in, like "People" for people/anna.md. */
		group: string | null;
		updatedAt: number;
		facts: MemoryFact[];
	}

	/** Most recently changed first. A folder's files stay together, where its newest one is. */
	export function orderTopics(topics: Topic[]): Topic[] {
		const newest = [...topics].sort((a, b) => b.updatedAt - a.updatedAt);
		const ordered: Topic[] = [];
		for (const topic of newest) {
			if (!topic.group) ordered.push(topic);
			else if (!ordered.some((t) => t.group === topic.group)) {
				ordered.push(...newest.filter((t) => t.group === topic.group));
			}
		}
		return ordered;
	}
</script>

<script lang="ts">
	import { formatAgo } from '$lib/format';

	interface Props {
		topics: Topic[];
		/** The topic pointed at, here or in the list of notes. */
		focus?: string | null;
		/** A topic's name was clicked. */
		onpick?: (path: string) => void;
	}

	let { topics, focus = $bindable(null), onpick }: Props = $props();

	/** Rows shown until "Show all". */
	const LIMIT = 12;
	const DAY = 24 * 60 * 60 * 1000;
	const now = Date.now();

	let showAll = $state(false);
	let box = $state<HTMLElement>();
	let active = $state<{ topic: Topic; fact: MemoryFact; x: number; y: number } | null>(null);

	const rows = $derived(orderTopics(topics));
	const shown = $derived(showAll || rows.length <= LIMIT + 2 ? rows : rows.slice(0, LIMIT));

	/** Oldest on the left, so each row reads like a little timeline. */
	function timeline(facts: MemoryFact[]): MemoryFact[] {
		return [...facts].sort((a, b) => (a.learnedAt ?? 0) - (b.learnedAt ?? 0));
	}

	/** How dark a dot is: 1 today, fading to 0.18 over about three months. Undated is lightest. */
	function shade(learnedAt: number | null): number {
		if (learnedAt === null) return 0.14;
		const days = Math.max(0, (now - learnedAt) / DAY);
		return 1 - 0.82 * Math.min(1, Math.log1p(days) / Math.log1p(90));
	}

	function ink(amount: number): string {
		return `color-mix(in oklab, var(--foreground) ${Math.round(amount * 100)}%, transparent)`;
	}

	function learned(at: number | null): string {
		return at === null ? 'learned a while ago' : `learned ${formatAgo(at, now)}`;
	}

	function show(event: Event, topic: Topic, fact: MemoryFact) {
		if (!box) return;
		const dot = (event.currentTarget as HTMLElement).getBoundingClientRect();
		const area = box.getBoundingClientRect();
		active = { topic, fact, x: dot.left + dot.width / 2 - area.left, y: dot.top - area.top };
	}

	const tooltipAlign = $derived.by(() => {
		if (!active || !box) return '-50%';
		const width = box.clientWidth;
		if (active.x < 130) return '-24px';
		if (active.x > width - 130) return 'calc(-100% + 24px)';
		return '-50%';
	});
</script>

<!-- A tap outside a dot puts the tooltip away. -->
<svelte:window
	onpointerdown={(event) => {
		if (active && !(event.target as Element).closest?.('[data-dot]')) active = null;
	}}
/>

<div bind:this={box} class="relative rounded-3xl border px-3 py-4 sm:px-6 sm:py-6">
	{#if rows.length}
		<div class="space-y-0.5">
			{#each shown as topic, r (topic.path)}
				{#if topic.group && topic.group !== shown[r - 1]?.group}
					<p
						class="px-3 pt-4 pb-1 text-xs font-medium tracking-wider text-muted-foreground uppercase first:pt-0"
					>
						{topic.group}
					</p>
				{/if}
				<div
					role="group"
					aria-label={topic.title}
					class="flex items-start gap-4 rounded-2xl px-3 py-3 transition-colors duration-200 sm:gap-6"
					class:bg-muted={focus === topic.path}
					onpointerenter={() => (focus = topic.path)}
					onpointerleave={() => (focus = null)}
				>
					<button
						type="button"
						class="w-24 shrink-0 truncate text-left text-[15px] leading-4 text-foreground/70 transition-colors hover:text-foreground sm:w-40"
						class:pl-3={topic.group}
						aria-label="{topic.title}: {topic.facts.length} {topic.facts.length === 1
							? 'memory'
							: 'memories'}. Show the note."
						onclick={() => onpick?.(topic.path)}>{topic.title}</button
					>
					<div class="flex min-h-4 flex-1 flex-wrap gap-2.5 sm:gap-3">
						{#each timeline(topic.facts) as fact, i (i)}
							<span
								data-dot
								aria-hidden="true"
								class="dot"
								class:new={fact.learnedAt !== null && now - fact.learnedAt < DAY}
								style:background-color={ink(shade(fact.learnedAt))}
								style:--delay="{Math.min(r * 70 + i * 30, 1400)}ms"
								onpointerenter={(event) =>
									event.pointerType === 'mouse' && show(event, topic, fact)}
								onpointerleave={(event) => event.pointerType === 'mouse' && (active = null)}
								onclick={(event) => show(event, topic, fact)}
							></span>
						{:else}
							<span class="pt-px text-xs text-muted-foreground">Nothing in it yet</span>
						{/each}
					</div>
				</div>
			{/each}
		</div>

		<div
			class="mt-4 flex flex-wrap items-center justify-between gap-3 px-3 text-xs text-muted-foreground"
		>
			{#if shown.length < rows.length || showAll}
				<button type="button" class="hover:text-foreground" onclick={() => (showAll = !showAll)}>
					{showAll ? 'Show fewer' : `Show all ${rows.length} topics`}
				</button>
			{:else}
				<span></span>
			{/if}
			<span class="flex items-center gap-1.5" aria-hidden="true">
				Older
				{#each [0.14, 0.3, 0.5, 0.75, 1] as amount (amount)}
					<span class="size-2 rounded-full" style:background-color={ink(amount)}></span>
				{/each}
				Newer
			</span>
		</div>
	{:else}
		<div class="flex flex-col items-center gap-4 px-6 py-12 text-center">
			<div class="flex gap-3" aria-hidden="true">
				{#each [0, 1, 2, 3, 4] as i (i)}
					<span class="size-4 rounded-full border border-dashed border-foreground/25"></span>
				{/each}
			</div>
			<p class="text-sm text-muted-foreground">
				Nothing remembered yet. Each thing btw learns becomes a dot here.
			</p>
		</div>
	{/if}

	{#if active}
		<div
			class="pointer-events-none absolute z-10 w-max max-w-72 rounded-2xl border bg-popover px-3 py-2 text-sm text-popover-foreground shadow-lg"
			style:left="{active.x}px"
			style:top="{active.y}px"
			style:transform="translate({tooltipAlign}, calc(-100% - 10px))"
		>
			<p class="mb-0.5 text-xs text-muted-foreground">
				{active.topic.title} · {learned(active.fact.learnedAt)}
			</p>
			<p class="line-clamp-4">{active.fact.text}</p>
		</div>
	{/if}
</div>

<style>
	.dot {
		position: relative;
		display: block;
		width: 1rem;
		height: 1rem;
		border-radius: 9999px;
		cursor: default;
		transition: transform 150ms ease-out;
		/* "backwards", not "both": a kept end state would override the hover scale. */
		animation: appear 500ms cubic-bezier(0.34, 1.56, 0.64, 1) var(--delay) backwards;
	}
	/* A bigger target than the dot, for fingers. */
	.dot::before {
		content: '';
		position: absolute;
		inset: -5px;
		border-radius: 9999px;
	}
	.dot:hover {
		transform: scale(1.3);
	}
	/* Learned today: a halo, and a slow ripple. */
	.dot.new {
		box-shadow:
			0 0 0 3px var(--background),
			0 0 0 4.5px color-mix(in oklab, var(--foreground) 14%, transparent);
	}
	.dot.new::after {
		content: '';
		position: absolute;
		inset: -4.5px;
		border-radius: 9999px;
		border: 1.5px solid color-mix(in oklab, var(--foreground) 30%, transparent);
		animation: ripple 3s ease-out calc(var(--delay) + 600ms) infinite;
		opacity: 0;
	}
	@keyframes appear {
		from {
			transform: scale(0);
			opacity: 0;
		}
	}
	@keyframes ripple {
		from {
			transform: scale(1);
			opacity: 0.8;
		}
		to {
			transform: scale(1.9);
			opacity: 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.dot,
		.dot.new::after {
			animation: none;
		}
	}
</style>
