<script lang="ts" module>
	/**
	 * What the sky is doing: nothing yet, stars coming out, the eight colors orbiting the
	 * wordmark, pooled into a glow behind the welcome, or gone.
	 */
	export type SkyStage = 'dark' | 'stars' | 'orbit' | 'aurora' | 'gone';
</script>

<script lang="ts">
	import { onMount } from 'svelte';
	import { AVATARS } from '@nolune/core/avatars';
	import { cn } from '$lib/utils';

	interface Props {
		stage: SkyStage;
		/**
		 * Deep space behind everything, whatever the theme. Turned off, it thins out to the page,
		 * leaving a few stars on a dark page and none on a light one.
		 */
		space?: boolean;
		/** Where the orbs start from (the wordmark's dots), in viewport pixels. */
		origins?: { x: number; y: number }[];
		/** What they circle, in viewport pixels. The middle of the screen without one. */
		center?: { x: number; y: number } | null;
		/** Jump to the stage's resting state instead of moving there (skipped, or reduced motion). */
		still?: boolean;
		class?: string;
	}

	let {
		stage,
		space = false,
		origins = [],
		center = null,
		still = false,
		class: className
	}: Props = $props();

	let canvas = $state<HTMLCanvasElement>();

	interface Orb {
		color: string;
		x: number;
		y: number;
		r: number;
		alpha: number;
	}

	interface Star {
		/** Across the screen at depth 1, from -1 to 1. */
		x: number;
		y: number;
		/** Depth: 1 far away, near 0 about to pass the camera. */
		z: number;
		size: number;
		color: string;
		/** When it comes out, in seconds after the stars begin. */
		appears: number;
		twinkle: number;
		phase: number;
	}

	/** Deep space: almost black, a little blue. */
	const SPACE = [4, 6, 14];
	/** Faint clouds far behind the stars. */
	const NEBULAE = [
		{ x: 0.22, y: 0.3, r: 0.55, color: '#2a3a8a', alpha: 0.2 },
		{ x: 0.78, y: 0.62, r: 0.6, color: '#5a2a7a', alpha: 0.16 },
		{ x: 0.55, y: 0.18, r: 0.45, color: '#1d5a6a', alpha: 0.14 }
	];
	/** The galaxy's band across the sky, tilted; many of the stars crowd along it. */
	const BAND = -0.42;
	const STAR_COLORS = ['#ffffff', '#ffffff', '#ffffff', '#fff1d6', '#d6e4ff', '#ffe1c2'];
	/** How fast the camera drifts into the stars, in depth per second: slow, like floating. */
	const DRIFT = 0.022;
	/** And how fast the whole field turns. */
	const TURN = 0.006;

	const TILT = (-11 * Math.PI) / 180;
	const clamp = (t: number) => Math.min(1, Math.max(0, t));
	const easeOut = (t: number) => 1 - (1 - clamp(t)) ** 3;
	const easeInOut = (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);

	/** `#70e0c4` at some opacity; canvas gradients don't take color-mix(). */
	function fade(hex: string, alpha: number): string {
		const m = hex.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i);
		if (!m) return alpha > 0.5 ? hex : 'transparent';
		const [r, g, b] = m.slice(1).map((h) => parseInt(h, 16));
		return `rgba(${r}, ${g}, ${b}, ${alpha})`;
	}

	onMount(() => {
		const el = canvas!;
		const ctx = el.getContext('2d');
		// The orbs' trails, kept apart so they fade without smearing the stars.
		const trailCanvas = document.createElement('canvas');
		const trails = trailCanvas.getContext('2d');
		if (!ctx || !trails) return;
		let width = 0;
		let height = 0;
		const resize = () => {
			const dpr = Math.min(2, window.devicePixelRatio || 1);
			width = window.innerWidth;
			height = window.innerHeight;
			for (const c of [el, trailCanvas]) {
				c.width = Math.round(width * dpr);
				c.height = Math.round(height * dpr);
			}
			ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
			trails.setTransform(dpr, 0, 0, dpr, 0, 0);
		};
		resize();
		window.addEventListener('resize', resize);

		// The avatars' colors as the page's theme has them.
		const style = getComputedStyle(document.documentElement);
		const colors = AVATARS.map((a) => style.getPropertyValue(`--avatar-${a}`).trim() || '#888');
		const dark = document.documentElement.classList.contains('dark');

		/** How far the camera has turned. */
		let turn = 0;
		/** Somewhere on screen at depth `z`, as the camera's turned now: crowded along the band. */
		const newStar = (appears: number, z = 0.15 + Math.random() * 0.85): Star => {
			const half = Math.max(width, height) / 2;
			let sx: number;
			let sy: number;
			if (Math.random() < 0.45) {
				const along = (Math.random() * 2 - 1) * 1.2;
				const across = (Math.random() + Math.random() + Math.random() - 1.5) * 0.12;
				sx = along * Math.cos(BAND) - across * Math.sin(BAND);
				sy = along * Math.sin(BAND) + across * Math.cos(BAND);
			} else {
				sx = ((Math.random() * 2 - 1) * width * 0.55) / half;
				sy = ((Math.random() * 2 - 1) * height * 0.55) / half;
			}
			// Undo the camera's turn, so it lands where it was meant to.
			const cos = Math.cos(-turn);
			const sin = Math.sin(-turn);
			return {
				x: (sx * cos - sy * sin) * z,
				y: (sx * sin + sy * cos) * z,
				z,
				size: 0.4 + Math.random() ** 3 * 1.6,
				color: STAR_COLORS[Math.floor(Math.random() * STAR_COLORS.length)],
				appears,
				twinkle: Math.random() < 0.4 ? 1.5 + Math.random() * 3 : 0,
				phase: Math.random() * Math.PI * 2
			};
		};
		// They come out over the first few seconds, the brightest first.
		const stars = Array.from({ length: Math.min(1500, Math.round((width * height) / 1000)) }, () =>
			newStar(0)
		)
			.sort((a, b) => b.size - a.size)
			.map((s, i, all) => ({ ...s, appears: (i / all.length) * 4.5 + Math.random() * 0.8 }));

		const orbs: Orb[] = colors.map((color) => ({
			color,
			x: width / 2,
			y: height / 2,
			r: 0,
			alpha: 0
		}));
		let current: SkyStage = 'dark';
		let since = performance.now();
		/** When the stars began. */
		let starsSince: number | null = null;
		let last = performance.now();
		/** How far deep space has given way to the page (0: all space, 1: all page). */
		let dawn = space ? 0 : 1;
		/** Where each orb was when the stage changed, to move on from there. */
		let from = orbs.map((o) => ({ ...o }));
		let shooting: { at: number; x: number; y: number; dx: number; dy: number } | null = null;
		let frame = 0;

		const mid = () => center ?? { x: width / 2, y: height * 0.42 };
		const radii = () => {
			const rx = Math.min(width * 0.47, 460);
			return { rx, ry: rx * 0.3 };
		};
		/** Planet `i`'s orbit, as a share of the widest: the first closest in. */
		const orbitOf = (i: number) => 0.55 + (0.45 * i) / (orbs.length - 1);
		function onEllipse(i: number, t: number, scale = orbitOf(i)) {
			const { x, y } = mid();
			const { rx, ry } = radii();
			// Closer in goes faster, as planets do; each starts somewhere else around.
			const a = i * 2.4 + t * 0.26 * scale ** -1.5;
			const ex = Math.cos(a) * rx * scale;
			const ey = Math.sin(a) * ry * scale;
			return {
				x: x + ex * Math.cos(TILT) - ey * Math.sin(TILT),
				y: y + ex * Math.sin(TILT) + ey * Math.cos(TILT)
			};
		}
		/** The orbits themselves, faint, like a map of them. */
		function drawOrbits(alpha: number) {
			if (alpha <= 0.001) return;
			const { x, y } = mid();
			const { rx, ry } = radii();
			ctx!.globalAlpha = alpha;
			ctx!.strokeStyle = '#ffffff';
			ctx!.lineWidth = 1;
			for (let i = 0; i < orbs.length; i++) {
				ctx!.beginPath();
				ctx!.ellipse(x, y, rx * orbitOf(i), ry * orbitOf(i), TILT, 0, Math.PI * 2);
				ctx!.stroke();
			}
		}
		/** Each color's place in the glow: spread wide and low behind the middle, drifting. */
		function inAurora(i: number, t: number) {
			const { x, y } = mid();
			const { rx } = radii();
			const spread = (i / (orbs.length - 1) - 0.5) * 1.5 * rx;
			return {
				x: x + spread + Math.sin(t * 0.25 + i * 1.3) * 30,
				y: y + (i % 2 ? -1 : 1) * 36 + Math.cos(t * 0.2 + i) * 22,
				r: Math.min(width, 1100) * (0.07 + (i % 3) * 0.018),
				// Brighter against space than against a light page.
				alpha: dark ? 0.22 : 0.14 + 0.1 * (1 - dawn)
			};
		}

		function drawOrb(c: CanvasRenderingContext2D, orb: Orb, solid = 0) {
			if (orb.alpha <= 0.001 || orb.r <= 0.1) return;
			const glow = c.createRadialGradient(orb.x, orb.y, 0, orb.x, orb.y, orb.r * 2.6);
			glow.addColorStop(0, fade(orb.color, 1));
			glow.addColorStop(0.3, fade(orb.color, 0.45));
			glow.addColorStop(1, fade(orb.color, 0));
			c.globalAlpha = orb.alpha;
			c.fillStyle = glow;
			c.beginPath();
			c.arc(orb.x, orb.y, orb.r * 2.6, 0, Math.PI * 2);
			c.fill();
			if (solid > 0) {
				// A planet: a solid body in its glow.
				c.globalAlpha = orb.alpha * solid;
				c.fillStyle = orb.color;
				c.beginPath();
				c.arc(orb.x, orb.y, orb.r * 0.7, 0, Math.PI * 2);
				c.fill();
			}
		}

		function drawSpace(t: number, fadeOut: number) {
			const spaceAmount = 1 - easeInOut(dawn);
			if (spaceAmount > 0.001) {
				ctx!.globalAlpha = spaceAmount;
				ctx!.fillStyle = `rgb(${SPACE.join(',')})`;
				ctx!.fillRect(0, 0, width, height);
				const size = Math.max(width, height);
				// The band: a long, faint glow the crowded stars sit in.
				ctx!.save();
				ctx!.translate(width / 2, height / 2);
				ctx!.rotate(BAND + turn);
				ctx!.scale(1, 0.16);
				const band = ctx!.createRadialGradient(0, 0, 0, 0, 0, size * 0.75);
				band.addColorStop(0, 'rgba(150, 165, 220, 1)');
				band.addColorStop(1, 'rgba(150, 165, 220, 0)');
				ctx!.globalAlpha = spaceAmount * 0.12 * clamp(t / 5) * fadeOut;
				ctx!.fillStyle = band;
				ctx!.beginPath();
				ctx!.arc(0, 0, size * 0.75, 0, Math.PI * 2);
				ctx!.fill();
				ctx!.restore();
				for (const n of NEBULAE) {
					const x = (n.x + Math.sin(t * 0.03 + n.r) * 0.03) * width;
					const y = (n.y + Math.cos(t * 0.025 + n.r) * 0.03) * height;
					const cloud = ctx!.createRadialGradient(x, y, 0, x, y, n.r * size);
					cloud.addColorStop(0, fade(n.color, 1));
					cloud.addColorStop(1, fade(n.color, 0));
					ctx!.globalAlpha = spaceAmount * n.alpha * clamp(t / 6) * fadeOut;
					ctx!.fillStyle = cloud;
					ctx!.fillRect(0, 0, width, height);
				}
			}

			// Stars stay on a dark page once space gives way; on a light one they go with it.
			const shown = (dark ? 0.35 + 0.65 * spaceAmount : spaceAmount) * fadeOut;
			if (shown <= 0.001) return;
			const cx = width / 2;
			const cy = height / 2;
			const half = Math.max(width, height) / 2;
			const cos = Math.cos(turn);
			const sin = Math.sin(turn);
			for (const s of stars) {
				const come = still ? 1 : clamp((t - s.appears) / 1.2);
				if (come <= 0) continue;
				const px = (s.x * cos - s.y * sin) / s.z;
				const py = (s.x * sin + s.y * cos) / s.z;
				const x = cx + px * half;
				const y = cy + py * half;
				if (x < -4 || x > width + 4 || y < -4 || y > height + 4) continue;
				const near = 1 - s.z;
				const twinkle = s.twinkle ? 0.7 + 0.3 * Math.sin(t * s.twinkle + s.phase) : 1;
				const alpha = shown * come * twinkle * (0.45 + 0.55 * near);
				const r = s.size * (0.6 + near * 1.1);
				ctx!.fillStyle = s.color;
				if (r >= 1.6) {
					// The brightest have a soft halo.
					const halo = ctx!.createRadialGradient(x, y, 0, x, y, r * 4);
					halo.addColorStop(0, fade(s.color, 0.35));
					halo.addColorStop(1, fade(s.color, 0));
					ctx!.globalAlpha = alpha;
					ctx!.fillStyle = halo;
					ctx!.fillRect(x - r * 4, y - r * 4, r * 8, r * 8);
					ctx!.fillStyle = s.color;
				}
				ctx!.globalAlpha = alpha;
				if (r < 0.9) ctx!.fillRect(x - r, y - r, r * 2, r * 2);
				else {
					ctx!.beginPath();
					ctx!.arc(x, y, r, 0, Math.PI * 2);
					ctx!.fill();
				}
			}

			// One shooting star, a few seconds in.
			if (!shooting && !still && t > 5.5 && t < 6) {
				shooting = {
					at: t,
					x: width * 0.18,
					y: height * 0.14,
					dx: width * 0.34,
					dy: height * 0.16
				};
			}
			if (shooting) {
				const k = (t - shooting.at) / 0.9;
				if (k >= 0 && k <= 1) {
					const head = easeInOut(k);
					const x = shooting.x + shooting.dx * head;
					const y = shooting.y + shooting.dy * head;
					const tail = 0.18;
					const tx = x - shooting.dx * tail;
					const ty = y - shooting.dy * tail;
					const streak = ctx!.createLinearGradient(tx, ty, x, y);
					streak.addColorStop(0, 'rgba(255,255,255,0)');
					streak.addColorStop(1, 'rgba(255,255,255,0.9)');
					ctx!.globalAlpha = shown * Math.sin(Math.PI * k);
					ctx!.strokeStyle = streak;
					ctx!.lineWidth = 1.4;
					ctx!.beginPath();
					ctx!.moveTo(tx, ty);
					ctx!.lineTo(x, y);
					ctx!.stroke();
				}
			}
		}

		function tick(now: number) {
			const dt = Math.min(0.1, (now - last) / 1000);
			last = now;
			if (stage !== current) {
				if (current === 'orbit') trails!.clearRect(0, 0, width, height);
				current = stage;
				since = now;
				from = orbs.map((o) => ({ ...o }));
			}
			if (current !== 'dark' && starsSince === null) starsSince = now;
			const t = (now - since) / 1000;
			const sky = starsSince === null ? 0 : (now - starsSince) / 1000;
			const jump = still;
			const out = current === 'gone' ? Math.max(0, 1 - t / 0.8) : 1;

			// The camera floats forward; stars that pass it come back far away.
			if (!jump) {
				turn += TURN * dt;
				for (const s of stars) {
					s.z -= DRIFT * dt;
					if (s.z < 0.06) Object.assign(s, newStar(0, 1), { appears: s.appears });
				}
			}
			// Space gives way to the page over a second and a half, like a sunrise.
			dawn = jump ? (space ? 0 : 1) : clamp(dawn + ((space ? -1 : 1) * dt) / 1.5);

			ctx!.globalAlpha = 1;
			ctx!.clearRect(0, 0, width, height);
			if (current !== 'dark') drawSpace(sky, out);

			if (current === 'orbit') {
				// Trails: what was drawn before fades a little each frame.
				trails!.globalCompositeOperation = 'destination-out';
				trails!.globalAlpha = 1;
				trails!.fillStyle = 'rgba(0,0,0,0.2)';
				trails!.fillRect(0, 0, width, height);
				trails!.globalCompositeOperation = 'source-over';
				const k = jump ? 1 : easeOut(t / 1.8);
				drawOrbits(0.07 * (jump ? 1 : clamp((t - 0.6) / 2)));
				orbs.forEach((orb, i) => {
					const target = onEllipse(i, t);
					const start = origins[i % Math.max(1, origins.length)] ?? mid();
					orb.x = start.x + (target.x - start.x) * k;
					orb.y = start.y + (target.y - start.y) * k;
					orb.r = 2.5 + (1.5 + (i % 3) * 1.2) * k;
					orb.alpha = Math.min(1, t * 4);
					drawOrb(trails!, orb, k);
				});
				ctx!.globalAlpha = 1;
				ctx!.drawImage(trailCanvas, 0, 0, width, height);
			} else if (current === 'aurora' || current === 'gone') {
				const k = jump ? 1 : easeOut(t / 2.4);
				orbs.forEach((orb, i) => {
					const target = inAurora(i, t);
					const start = from[i];
					orb.x = start.x + (target.x - start.x) * k;
					orb.y = start.y + (target.y - start.y) * k;
					orb.r = start.r + (target.r - start.r) * k;
					orb.alpha = (start.alpha + (target.alpha - start.alpha) * k) * out;
					// The planets melt into the glow.
					drawOrb(ctx!, orb, 1 - k);
				});
			}
			if (current === 'gone' && out === 0) return;
			frame = requestAnimationFrame(tick);
		}
		// Runs until the sky is gone; the stage is read each frame.
		frame = requestAnimationFrame(tick);

		return () => {
			cancelAnimationFrame(frame);
			window.removeEventListener('resize', resize);
		};
	});
</script>

<canvas
	bind:this={canvas}
	aria-hidden="true"
	class={cn('pointer-events-none fixed inset-0 size-full', className)}
></canvas>
