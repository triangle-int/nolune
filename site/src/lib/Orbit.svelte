<script lang="ts">
	import { random } from './random';

	// The hero picture: nolune as a small planet, its three dots on the face, with a ship and two
	// moons going round. Everything moves on ellipses tilted the same way, worked out per frame.

	const C = { x: 320, y: 250 };
	const TILT = (-22 * Math.PI) / 180;
	const SQUASH = 0.42;
	const PLANET = 64;

	/** A point on the orbit of radius `r`, at angle `t`. */
	function at(r: number, t: number) {
		const [x, y] = [r * Math.cos(t), SQUASH * r * Math.sin(t)];
		return {
			x: C.x + x * Math.cos(TILT) - y * Math.sin(TILT),
			y: C.y + x * Math.sin(TILT) + y * Math.cos(TILT)
		};
	}

	function arc(r: number, from: number, to: number): string {
		const steps = 28;
		return Array.from({ length: steps + 1 }, (_, i) => {
			const p = at(r, from + ((to - from) * i) / steps);
			return `${i ? 'L' : 'M'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`;
		}).join('');
	}

	const orbits = [
		{ r: 205, kind: 'solid' },
		{ r: 240, kind: 'faint' },
		{ r: 275, kind: 'dashed' }
	];

	const dust = (() => {
		const next = random(7);
		return Array.from({ length: 80 }, () => {
			const angle = next() * Math.PI * 2;
			const distance = PLANET + 8 + next() ** 1.6 * 100;
			return {
				x: C.x + Math.cos(angle) * distance,
				y: C.y + Math.sin(angle) * distance,
				r: 0.5 + next() * 0.8,
				opacity: 0.12 + next() * 0.4
			};
		});
	})();

	const sparkles = [
		{ x: 590, y: 38, s: 15, delay: 0 },
		{ x: 128, y: 96, s: 6, delay: 1.3 },
		{ x: 470, y: 118, s: 5, delay: 2.1 },
		{ x: 70, y: 318, s: 5, delay: 0.7 },
		{ x: 610, y: 262, s: 4, delay: 2.8 }
	];

	function sparkle(s: number): string {
		return `M0 ${-s}Q0 0 ${s} 0Q0 0 0 ${s}Q0 0 ${-s} 0Q0 0 0 ${-s}Z`;
	}

	// Seconds since the page loaded. It stays at 0 without script or with reduced motion.
	let time = $state(0);

	$effect(() => {
		if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
		let frame = requestAnimationFrame(function tick(now) {
			time = now / 1000;
			frame = requestAnimationFrame(tick);
		});
		return () => cancelAnimationFrame(frame);
	});

	// The ship goes round the near side from left to right, its trail behind it.
	const SHIP_R = 222;
	const TRAIL = 2.7;
	const ship = $derived(-0.45 - (time / 26) * Math.PI * 2);
	const shipAt = $derived(at(SHIP_R, ship));
	const moons = $derived([
		{ ...at(240, 2.3 - (time / 60) * Math.PI * 2), r: 8 },
		{ ...at(275, 4.6 - (time / 95) * Math.PI * 2), r: 11 }
	]);
</script>

<div class="orbit">
	<svg viewBox="30 30 590 440" aria-hidden="true">
		<defs>
			<radialGradient id="orbit-halo">
				<stop offset="0" stop-color="#3f6fb8" stop-opacity="0.4" />
				<stop offset="0.45" stop-color="#23467f" stop-opacity="0.18" />
				<stop offset="1" stop-color="#23467f" stop-opacity="0" />
			</radialGradient>
			<radialGradient id="orbit-planet" cx="0.38" cy="0.32" r="0.8">
				<stop offset="0" stop-color="#fdf9f0" />
				<stop offset="0.55" stop-color="#e6dfcf" />
				<stop offset="1" stop-color="#aea590" />
			</radialGradient>
			<radialGradient id="orbit-moon" cx="0.35" cy="0.3" r="0.8">
				<stop offset="0" stop-color="#d9dee8" />
				<stop offset="1" stop-color="#6d778a" />
			</radialGradient>
			<radialGradient id="orbit-glow">
				<stop offset="0" stop-color="#ff9b5e" stop-opacity="0.9" />
				<stop offset="0.35" stop-color="#f07a3c" stop-opacity="0.35" />
				<stop offset="1" stop-color="#f07a3c" stop-opacity="0" />
			</radialGradient>
		</defs>

		<circle cx={C.x} cy={C.y} r="175" fill="url(#orbit-halo)" />
		{#each dust as d, i (i)}
			<circle cx={d.x} cy={d.y} r={d.r} opacity={d.opacity} class="dust" />
		{/each}

		{#each orbits as o (o.r)}
			<ellipse
				cx={C.x}
				cy={C.y}
				rx={o.r}
				ry={o.r * SQUASH}
				transform="rotate({(TILT * 180) / Math.PI} {C.x} {C.y})"
				class="ring {o.kind}"
			/>
		{/each}

		{#each moons as moon, i (i)}
			<circle cx={moon.x} cy={moon.y} r={moon.r} fill="url(#orbit-moon)" />
		{/each}

		<circle cx={C.x} cy={C.y} r={PLANET} fill="url(#orbit-planet)" />
		<g class="craters">
			<circle cx={C.x - 30} cy={C.y - 34} r="7" />
			<circle cx={C.x + 34} cy={C.y + 28} r="10" />
			<circle cx={C.x + 22} cy={C.y - 40} r="4" />
			<circle cx={C.x - 38} cy={C.y + 30} r="5" />
		</g>
		<g class="face">
			<circle cx={C.x - 23} cy={C.y + 2} r="9" />
			<circle cx={C.x} cy={C.y + 2} r="9" />
			<circle cx={C.x + 23} cy={C.y + 2} r="9" />
		</g>

		<path d={arc(SHIP_R, ship + TRAIL, ship)} class="trail" opacity="0.25" />
		<path d={arc(SHIP_R, ship + TRAIL * 0.6, ship)} class="trail" opacity="0.55" />
		<path d={arc(SHIP_R, ship + TRAIL * 0.3, ship)} class="trail" />
		<circle cx={shipAt.x} cy={shipAt.y} r="18" fill="url(#orbit-glow)" />
		<circle cx={shipAt.x} cy={shipAt.y} r="3.4" class="ship" />

		{#each sparkles as s, i (i)}
			<!-- The group places it: the twinkle's CSS transform would replace a transform here. -->
			<g transform="translate({s.x} {s.y})">
				<path d={sparkle(s.s)} class="sparkle" style:animation-delay="{s.delay}s" />
			</g>
		{/each}
	</svg>
	<p class="label">Mission<br />home</p>
</div>

<style>
	.orbit {
		position: relative;
		width: 100%;
	}
	svg {
		display: block;
		width: 100%;
		height: auto;
		overflow: visible;
	}
	.dust {
		fill: #c9d6ee;
	}
	.ring {
		fill: none;
		stroke: var(--cream);
	}
	.solid {
		stroke-width: 1.6;
		opacity: 0.85;
	}
	.faint {
		stroke: #97acc9;
		stroke-width: 1;
		opacity: 0.45;
	}
	.dashed {
		stroke-width: 1.4;
		stroke-dasharray: 2 7;
		stroke-linecap: round;
		opacity: 0.7;
	}
	.craters {
		fill: #cdc4b0;
		opacity: 0.55;
	}
	.face {
		fill: var(--ink);
	}
	.trail {
		fill: none;
		stroke: var(--rust);
		stroke-width: 2.4;
		stroke-linecap: round;
	}
	.ship {
		fill: #ffe2c8;
	}
	.sparkle {
		fill: var(--cream);
		transform-box: fill-box;
		transform-origin: center;
		animation: twinkle 4.5s ease-in-out infinite;
	}
	.label {
		position: absolute;
		right: 0;
		bottom: 6%;
		margin: 0;
		font: 500 0.75rem/1.4 var(--mono);
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--muted);
	}
	.label::after {
		content: '';
		display: block;
		width: 1.25rem;
		height: 1px;
		margin-top: 0.5rem;
		background: currentColor;
	}

	@keyframes twinkle {
		50% {
			opacity: 0.35;
			transform: scale(0.7);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.sparkle {
			animation: none;
		}
	}
</style>
