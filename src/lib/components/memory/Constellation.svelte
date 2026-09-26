<script lang="ts" module>
	export interface Topic {
		/** The file, relative to /memories. */
		path: string;
		title: string;
		facts: string[];
		hue: number;
		/** Changed in the last day. */
		fresh: boolean;
	}

	/** A stable number in [0, 1) per string, so the sky looks the same on every visit. */
	function hash(text: string): number {
		let h = 2166136261;
		for (let i = 0; i < text.length; i++) {
			h ^= text.charCodeAt(i);
			h = Math.imul(h, 16777619);
		}
		// Mix the bits, or keys that differ only at the end (dust-1, dust-2…) land in a line.
		h ^= h >>> 16;
		h = Math.imul(h, 0x85ebca6b);
		h ^= h >>> 13;
		h = Math.imul(h, 0xc2b2ae35);
		h ^= h >>> 16;
		return (h >>> 0) / 4294967296;
	}
</script>

<script lang="ts">
	import { prefersReducedMotion } from 'svelte/motion';

	interface Props {
		topics: Topic[];
		/** The profile, drawn in the middle. */
		name: string;
		/** The topic highlighted here and in the list. */
		focus?: string | null;
		/** A topic was clicked. */
		onpick?: (path: string) => void;
	}

	let { topics, name, focus = $bindable(null), onpick }: Props = $props();

	/** Stars drawn per topic; the list below has every fact. */
	const MAX_STARS = 40;

	let width = $state(0);
	let height = $state(0);
	let visible = $state(true);
	/** Seconds of motion so far. Stands still while something is pointed at, so it can be read. */
	let t = $state(0);
	let active = $state<{ topic: number; fact: number | null } | null>(null);

	const center = $derived({ x: width / 2, y: height / 2 });

	const planets = $derived.by(() => {
		const n = topics.length;
		if (!n || !width) return [];
		// Roughly the room each topic has along its ring, which decides how wide its stars can spread.
		const room = (Math.PI * ((width + height) / 2 - 120)) / n;
		const orbit = Math.max(16, Math.min(48, room * 0.3, height * 0.15));
		const rx = Math.max(0, width / 2 - orbit - 30);
		const ry = Math.max(0, height / 2 - orbit - 24);
		// Odd counts start at the top; even ones sit level, which suits a wide box.
		const start = n % 2 ? -Math.PI / 2 : 0;
		return topics.map((topic, i) => {
			const angle = start + (2 * Math.PI * i) / n;
			const facts = topic.facts.slice(0, MAX_STARS);
			const seed = hash(topic.path);
			return {
				...topic,
				x: center.x + (n === 1 ? 0 : rx * Math.cos(angle)),
				y: center.y + (n === 1 ? -ry * 0.55 : ry * Math.sin(angle)),
				r: Math.min(13, 6 + Math.sqrt(topic.facts.length) * 1.4),
				phase: seed * Math.PI * 2,
				spin: (i % 2 ? -1 : 1) * (0.05 + seed * 0.04),
				label: topic.title.length > 20 ? `${topic.title.slice(0, 19)}…` : topic.title,
				stars: facts.map((fact, j) => {
					const s = hash(`${topic.path}#${j}`);
					// Many facts get two rings so they don't pile up.
					const ring = facts.length > 14 ? (j % 2 ? 0.66 : 1) : 0.8 + s * 0.2;
					return {
						angle: (2 * Math.PI * j) / facts.length + s * 0.5,
						distance: orbit * ring,
						size: 1.7 + Math.min(1.9, fact.length / 70),
						twinkle: s
					};
				})
			};
		});
	});

	const dust = $derived(
		Array.from({ length: width ? Math.round((width * height) / 5500) : 0 }, (_, i) => ({
			x: hash(`dust-x${i}`) * width,
			y: hash(`dust-y${i}`) * height,
			r: 0.5 + hash(`dust-r${i}`) * 0.8,
			delay: hash(`dust-d${i}`) * 6
		}))
	);

	function planetAt(p: (typeof planets)[number]) {
		return { x: p.x, y: p.y + Math.sin(t * 0.6 + p.phase) * 2.5 };
	}

	function starAt(p: (typeof planets)[number], s: (typeof p.stars)[number]) {
		const home = planetAt(p);
		const a = s.angle + t * p.spin;
		return { x: home.x + Math.cos(a) * s.distance, y: home.y + Math.sin(a) * s.distance };
	}

	$effect(() => {
		if (prefersReducedMotion.current || !visible) return;
		let last = performance.now();
		let frame = requestAnimationFrame(function tick(now) {
			if (!active) t += Math.min(0.05, (now - last) / 1000);
			last = now;
			frame = requestAnimationFrame(tick);
		});
		return () => cancelAnimationFrame(frame);
	});

	/** No motion while scrolled out of view. */
	function watchVisibility(node: HTMLElement) {
		const observer = new IntersectionObserver(([entry]) => (visible = entry.isIntersecting));
		observer.observe(node);
		return () => observer.disconnect();
	}

	const tooltip = $derived.by(() => {
		if (!active) return null;
		const p = planets[active.topic];
		if (!p) return null;
		const at = active.fact === null ? planetAt(p) : starAt(p, p.stars[active.fact]);
		return {
			x: at.x,
			y: at.y,
			hue: p.hue,
			title: p.title,
			text:
				active.fact === null
					? `${p.facts.length} ${p.facts.length === 1 ? 'memory' : 'memories'}`
					: p.facts[active.fact],
			// Keep it inside the box: flip below near the top, lean inward near the sides.
			below: at.y < 90,
			align: at.x < width * 0.25 ? 'left' : at.x > width * 0.75 ? 'right' : 'center'
		};
	});

	function point(topic: number, fact: number | null) {
		active = { topic, fact };
		focus = planets[topic]?.path ?? null;
	}

	function leave() {
		active = null;
		focus = null;
	}

	const total = $derived(topics.reduce((sum, topic) => sum + topic.facts.length, 0));
</script>

<div
	class="relative h-[340px] w-full overflow-hidden rounded-3xl border bg-muted/30 select-none sm:h-[400px]"
	bind:clientWidth={width}
	bind:clientHeight={height}
	{@attach watchVisibility}
>
	{#if width}
		<svg
			viewBox="0 0 {width} {height}"
			class="absolute inset-0 size-full"
			role="img"
			aria-label="A map of what btw remembers: {topics.length} topics, {total} memories."
		>
			<!-- Tapping empty sky puts a tooltip away. -->
			<rect {width} {height} fill="transparent" aria-hidden="true" onclick={leave} />
			{#each dust as d, i (i)}
				<circle cx={d.x} cy={d.y} r={d.r} class="dust" style:--delay="{d.delay}s" />
			{/each}

			{#each planets as p, i (p.path)}
				{@const home = planetAt(p)}
				<path
					d="M {center.x} {center.y} L {home.x} {home.y}"
					pathLength="1"
					class="link draw"
					class:dim={focus && focus !== p.path}
					style:--delay="{150 + i * 90}ms"
				/>
			{/each}

			<g class="pop" style:--delay="0ms">
				<circle cx={center.x} cy={center.y} r="22" class="pulse core-ring" />
				<circle cx={center.x} cy={center.y} r="22" class="pulse core-ring" style:--delay="1.6s" />
				<circle cx={center.x} cy={center.y} r="22" class="core" />
				<text x={center.x} y={center.y} dy="0.35em" text-anchor="middle" class="core-initial">
					{name.trim().charAt(0).toUpperCase() || '?'}
				</text>
			</g>

			{#each planets as p, i (p.path)}
				{@const home = planetAt(p)}
				<g
					style:--h={p.hue}
					class="topic"
					class:dim={focus && focus !== p.path}
					class:lit={focus === p.path}
				>
					{#each p.stars as s, j (j)}
						{@const at = starAt(p, s)}
						<line x1={home.x} y1={home.y} x2={at.x} y2={at.y} class="spoke" />
					{/each}
					{#each p.stars as s, j (j)}
						{@const at = starAt(p, s)}
						<circle
							cx={at.x}
							cy={at.y}
							r={active?.topic === i && active.fact === j ? s.size + 2 : s.size}
							class="star"
							style:--delay="{600 + i * 90 + j * 22}ms"
							style:--twinkle="{2.5 + s.twinkle * 3}s"
						/>
						<!-- A bigger invisible target: the stars themselves are hard to point at. -->
						<circle
							cx={at.x}
							cy={at.y}
							r="9"
							class="hit"
							aria-hidden="true"
							onpointerenter={(event) => event.pointerType === 'mouse' && point(i, j)}
							onpointerleave={(event) => event.pointerType === 'mouse' && leave()}
							onclick={() => point(i, j)}
						/>
					{/each}

					<g
						role="button"
						tabindex="0"
						aria-label="{p.title}: {p.facts.length} {p.facts.length === 1
							? 'memory'
							: 'memories'}. Show the note."
						class="planet-button"
						onpointerenter={(event) => event.pointerType === 'mouse' && point(i, null)}
						onpointerleave={(event) => event.pointerType === 'mouse' && leave()}
						onfocus={() => point(i, null)}
						onblur={leave}
						onclick={() => onpick?.(p.path)}
						onkeydown={(event) => {
							if (event.key === 'Enter' || event.key === ' ') {
								event.preventDefault();
								onpick?.(p.path);
							}
						}}
					>
						{#if p.fresh}
							<circle cx={home.x} cy={home.y} r={p.r} class="pulse fresh-ring" />
						{/if}
						<circle
							cx={home.x}
							cy={home.y}
							r={p.r * 2.2}
							class="halo pop"
							style:--delay="{380 + i * 90}ms"
						/>
						<circle
							cx={home.x}
							cy={home.y}
							r={p.r}
							class="planet pop"
							style:--delay="{380 + i * 90}ms"
						/>
					</g>
					<text
						x={home.x}
						y={home.y + p.r * 2.2 + 12}
						text-anchor="middle"
						class="label fade"
						style:--delay="{700 + i * 90}ms">{p.label}</text
					>
				</g>
			{/each}
		</svg>

		{#if tooltip}
			<div
				class="pointer-events-none absolute z-10 w-max max-w-64 rounded-2xl border bg-popover px-3 py-2 text-sm text-popover-foreground shadow-lg"
				style:left="{tooltip.x}px"
				style:top="{tooltip.y}px"
				style:transform="translate({tooltip.align === 'left'
					? '-12px'
					: tooltip.align === 'right'
						? 'calc(-100% + 12px)'
						: '-50%'}, {tooltip.below ? '16px' : 'calc(-100% - 16px)'})"
			>
				<div class="mb-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
					<span class="size-2 rounded-full" style:background-color="hsl({tooltip.hue} 65% 55%)"
					></span>
					{tooltip.title}
				</div>
				<p class="line-clamp-4">{tooltip.text}</p>
			</div>
		{/if}

		{#if !topics.length}
			<p
				class="pointer-events-none absolute inset-x-0 bottom-8 px-6 text-center text-sm text-muted-foreground"
			>
				Nothing remembered yet. Things btw learns will show up here as stars.
			</p>
		{/if}
	{/if}
</div>

<style>
	.dust {
		fill: var(--muted-foreground);
		opacity: 0.25;
		animation: twinkle 4s ease-in-out infinite;
		animation-delay: var(--delay);
	}
	.link {
		fill: none;
		stroke: color-mix(in oklab, var(--foreground) 14%, transparent);
		stroke-width: 1;
		stroke-dasharray: 1;
		transition: opacity 300ms;
	}
	.core {
		fill: var(--foreground);
	}
	.core-initial {
		fill: var(--background);
		font-size: 16px;
		font-weight: 600;
	}
	.core-ring {
		fill: none;
		stroke: var(--foreground);
		stroke-width: 1;
	}
	.topic {
		transition: opacity 300ms;
	}
	.dim {
		opacity: 0.2;
	}
	.spoke {
		stroke: hsl(var(--h) 60% 55% / 0.18);
		stroke-width: 0.75;
		transition: stroke 300ms;
	}
	.lit .spoke {
		stroke: hsl(var(--h) 60% 55% / 0.5);
	}
	.star {
		fill: hsl(var(--h) 70% 58%);
		transform-box: fill-box;
		transform-origin: center;
		animation:
			pop 600ms cubic-bezier(0.34, 1.56, 0.64, 1) var(--delay) both,
			glint var(--twinkle) ease-in-out calc(var(--delay) + 600ms) infinite;
		transition: r 150ms;
	}
	.hit {
		fill: transparent;
		cursor: pointer;
	}
	.halo {
		fill: hsl(var(--h) 70% 58% / 0.14);
	}
	.planet {
		fill: hsl(var(--h) 65% 52%);
	}
	.fresh-ring {
		fill: none;
		stroke: hsl(var(--h) 65% 52%);
		stroke-width: 1.5;
	}
	.planet-button {
		cursor: pointer;
		outline: none;
	}
	.planet-button:focus-visible .planet {
		stroke: var(--foreground);
		stroke-width: 2;
	}
	.label {
		fill: var(--foreground);
		font-size: 12px;
		font-weight: 500;
		paint-order: stroke;
		stroke: color-mix(in oklab, var(--muted) 30%, var(--background));
		stroke-width: 4px;
		stroke-linejoin: round;
		pointer-events: none;
	}
	:global(.dark) .star {
		fill: hsl(var(--h) 80% 72%);
	}
	:global(.dark) .planet {
		fill: hsl(var(--h) 70% 64%);
	}
	:global(.dark) .halo {
		fill: hsl(var(--h) 80% 70% / 0.14);
	}

	/* Entrance: lines draw out from the middle, then topics and their stars pop in. */
	.pop {
		transform-box: fill-box;
		transform-origin: center;
		animation: pop 600ms cubic-bezier(0.34, 1.56, 0.64, 1) var(--delay) both;
	}
	.draw {
		animation: draw 700ms ease-out var(--delay) both;
	}
	.fade {
		animation: fade 500ms ease-out var(--delay) both;
	}
	.pulse {
		transform-box: fill-box;
		transform-origin: center;
		animation: pulse 3.2s ease-out var(--delay, 0s) infinite;
	}
	@keyframes pop {
		from {
			transform: scale(0);
			opacity: 0;
		}
	}
	@keyframes draw {
		from {
			stroke-dashoffset: 1;
		}
		to {
			stroke-dashoffset: 0;
		}
	}
	@keyframes fade {
		from {
			opacity: 0;
		}
	}
	@keyframes pulse {
		from {
			transform: scale(1);
			opacity: 0.45;
		}
		to {
			transform: scale(2.4);
			opacity: 0;
		}
	}
	@keyframes twinkle {
		50% {
			opacity: 0.05;
		}
	}
	@keyframes glint {
		50% {
			fill-opacity: 0.45;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.dust,
		.star,
		.pop,
		.draw,
		.fade,
		.pulse {
			animation: none;
		}
		.pulse {
			display: none;
		}
	}
</style>
