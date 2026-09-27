<script lang="ts" module>
	/** What the sky is doing: nothing yet, orbiting the wordmark, pooled into an aurora, or gone. */
	export type SkyStage = 'dark' | 'orbit' | 'aurora' | 'gone';
</script>

<script lang="ts">
	import { onMount } from 'svelte';
	import { AVATARS } from '@btw/core/avatars';
	import { cn } from '$lib/utils';

	interface Props {
		stage: SkyStage;
		/** Where the orbs start from (the wordmark's dots), in viewport pixels. */
		origins?: { x: number; y: number }[];
		/** What they circle, in viewport pixels. The middle of the screen without one. */
		center?: { x: number; y: number } | null;
		/** Jump to the stage's resting state instead of moving there (skipped, or reduced motion). */
		still?: boolean;
		class?: string;
	}

	let { stage, origins = [], center = null, still = false, class: className }: Props = $props();

	let canvas = $state<HTMLCanvasElement>();

	interface Orb {
		color: string;
		x: number;
		y: number;
		r: number;
		alpha: number;
	}

	const TILT = (-11 * Math.PI) / 180;
	const easeOut = (t: number) => 1 - (1 - Math.min(1, Math.max(0, t))) ** 3;

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
		if (!ctx) return;
		let width = 0;
		let height = 0;
		const resize = () => {
			const dpr = Math.min(2, window.devicePixelRatio || 1);
			width = window.innerWidth;
			height = window.innerHeight;
			el.width = Math.round(width * dpr);
			el.height = Math.round(height * dpr);
			ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		};
		resize();
		window.addEventListener('resize', resize);

		// The avatars' colors as the page's theme has them.
		const style = getComputedStyle(document.documentElement);
		const colors = AVATARS.map((a) => style.getPropertyValue(`--avatar-${a}`).trim() || '#888');
		const dark = document.documentElement.classList.contains('dark');

		const orbs: Orb[] = colors.map((color) => ({
			color,
			x: width / 2,
			y: height / 2,
			r: 0,
			alpha: 0
		}));
		let current: SkyStage = 'dark';
		let since = performance.now();
		/** Where each orb was when the stage changed, to move on from there. */
		let from = orbs.map((o) => ({ ...o }));
		let frame = 0;

		const mid = () => center ?? { x: width / 2, y: height * 0.42 };
		const radii = () => {
			const rx = Math.min(width * 0.34, 460);
			return { rx, ry: rx * 0.3 };
		};
		function onEllipse(i: number, t: number) {
			const { x, y } = mid();
			const { rx, ry } = radii();
			const a = (i / orbs.length) * Math.PI * 2 + t * 0.9;
			const ex = Math.cos(a) * rx;
			const ey = Math.sin(a) * ry;
			return {
				x: x + ex * Math.cos(TILT) - ey * Math.sin(TILT),
				y: y + ex * Math.sin(TILT) + ey * Math.cos(TILT)
			};
		}
		/** Each color's place in the aurora: spread wide and low behind the middle, drifting. */
		function inAurora(i: number, t: number) {
			const { x, y } = mid();
			const { rx } = radii();
			const spread = (i / (orbs.length - 1) - 0.5) * 1.5 * rx;
			return {
				x: x + spread + Math.sin(t * 0.25 + i * 1.3) * 30,
				y: y + (i % 2 ? -1 : 1) * 36 + Math.cos(t * 0.2 + i) * 22,
				r: Math.min(width, 1100) * (0.07 + (i % 3) * 0.018),
				alpha: dark ? 0.22 : 0.14
			};
		}

		function draw(orb: Orb) {
			if (orb.alpha <= 0.001 || orb.r <= 0.1) return;
			const glow = ctx!.createRadialGradient(orb.x, orb.y, 0, orb.x, orb.y, orb.r * 2.6);
			glow.addColorStop(0, fade(orb.color, 1));
			glow.addColorStop(0.3, fade(orb.color, 0.45));
			glow.addColorStop(1, fade(orb.color, 0));
			ctx!.globalAlpha = orb.alpha;
			ctx!.fillStyle = glow;
			ctx!.beginPath();
			ctx!.arc(orb.x, orb.y, orb.r * 2.6, 0, Math.PI * 2);
			ctx!.fill();
		}

		function tick(now: number) {
			if (stage !== current) {
				current = stage;
				since = now;
				from = orbs.map((o) => ({ ...o }));
			}
			const t = (now - since) / 1000;
			const jump = still;

			if (current === 'orbit') {
				// Trails: what was drawn before fades a little each frame.
				ctx!.globalCompositeOperation = 'destination-out';
				ctx!.globalAlpha = 1;
				ctx!.fillStyle = 'rgba(0,0,0,0.16)';
				ctx!.fillRect(0, 0, width, height);
				ctx!.globalCompositeOperation = 'source-over';
				orbs.forEach((orb, i) => {
					const target = onEllipse(i, t);
					const start = origins[i % Math.max(1, origins.length)] ?? mid();
					const k = jump ? 1 : easeOut(t / 0.9);
					orb.x = start.x + (target.x - start.x) * k;
					orb.y = start.y + (target.y - start.y) * k;
					orb.r = 3 + 4 * k;
					orb.alpha = Math.min(1, t * 6);
					draw(orb);
				});
			} else if (current === 'aurora' || current === 'gone') {
				ctx!.clearRect(0, 0, width, height);
				const k = jump ? 1 : easeOut(t / 1.4);
				const out = current === 'gone' ? Math.max(0, 1 - t / 0.8) : 1;
				orbs.forEach((orb, i) => {
					const target = inAurora(i, t);
					const start = from[i];
					orb.x = start.x + (target.x - start.x) * k;
					orb.y = start.y + (target.y - start.y) * k;
					orb.r = start.r + (target.r - start.r) * k;
					orb.alpha = (start.alpha + (target.alpha - start.alpha) * k) * out;
					draw(orb);
				});
				if (current === 'gone' && out === 0) return;
			} else {
				ctx!.clearRect(0, 0, width, height);
			}
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
