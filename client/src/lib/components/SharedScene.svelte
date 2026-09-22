<script lang="ts">
	import { onMount, onDestroy } from "svelte";
	import { getSceneStore } from "$lib/stores/scene.svelte.js";
	import MoonExpression from "$lib/components/companion/MoonExpression.svelte";

	const store = getSceneStore();

	let container: HTMLDivElement | undefined = $state();

	function easeOutCubic(x: number) { return 1 - Math.pow(1 - x, 3); }
	function easeInOutQuart(x: number) {
		return x < 0.5 ? 8 * x * x * x * x : 1 - Math.pow(-2 * x + 2, 4) / 2;
	}

	// ── Orb state ──
	interface OrbState {
		slug: string;
		// Position as % of container (50 = center)
		x: number;
		y: number;
		// Size in px
		size: number;
		opacity: number;
		visible: boolean;
	}

	let orbs = $state<OrbState[]>([]);
	let raf: number;
	let prevTime = 0;
	let elapsed = 0;
	let lastMode = '';

	// Convert 3D world X coord to CSS left% (camera at z=5, FOV 50)
	// At z=5 with FOV 50, visible width ≈ 2 * 5 * tan(25°) ≈ 4.66
	// So worldX=1.8 → 50% + 1.8/4.66*50% ≈ 69.3%
	const WORLD_TO_PCT = 50 / 4.66; // ~10.73% per world unit

	// Scale orbs to viewport — 450px at 900px+ height, shrinks for smaller windows
	function baseSize(): number {
		const h = container?.clientHeight ?? 900;
		return Math.min(450, h * 0.5);
	}

	const HOME_SCALE = 0.5;
	const FINAL_SCALE = 1.2;

	function animate() {
		raf = requestAnimationFrame(animate);

		const now = performance.now();
		const delta = (now - prevTime) / 1000;
		prevTime = now;
		if (delta > 0.1) return; // skip large gaps

		store.tick();

		const m = store.mode;
		const sel = store.selectedSlug;
		// One companion per server: the only orb is the selected companion.
		const slugs: string[] = sel ? [sel] : [];

		// Ensure orbs array matches slugs
		const existing = new Map(orbs.map(o => [o.slug, o]));
		const newOrbs: OrbState[] = slugs.map(slug => {
			return existing.get(slug) ?? { slug, x: 50, y: 50, size: 0, opacity: 0, visible: false };
		});

		const homePositions = new Map<string, number>();

		const useLerp = m === "home" || m === "chat" || m === "onboarding";
		const lerpF = Math.min(delta * 6, 1);

		for (const orb of newOrbs) {
			const isSelected = orb.slug === sel;
			const isHovered = false;
			const homeX = homePositions.get(orb.slug) ?? 0;

			let tx = 50 + homeX * WORLD_TO_PCT;
			let ty = 50;
			let ts = HOME_SCALE * baseSize();
			let to = 1;

			if (m === "home") {
				ts = (isHovered ? 0.58 : HOME_SCALE) * baseSize();
			} else if (m === "onboarding") {
				if (isSelected) {
					tx = 50; ty = 50;
					// Full viewport background - use container size
					ts = baseSize();
					to = 0.6;
				} else {
					ts = 0; to = 0;
				}
			} else if (m === "selecting") {
				const e = easeInOutQuart(store.selectProgress);
				if (isSelected) {
					const hx = 50 + homeX * WORLD_TO_PCT;
					tx = hx + (50 - hx) * e;
					ty = 50;
					ts = (HOME_SCALE + e * (FINAL_SCALE - HOME_SCALE)) * baseSize();
				} else {
					ts = HOME_SCALE * (1 - e) * baseSize();
					to = 1 - e;
				}
			} else if (m === "intro") {
				if (isSelected) {
					const p = store.introProgress;
					tx = 50; ty = 50;
					if (p < 0.25) {
						const e = easeOutCubic(p / 0.25);
						ts = (FINAL_SCALE + e * 0.15) * baseSize();
					} else if (p < 0.58) {
						const e = easeInOutQuart((p - 0.25) / 0.33);
						ts = (FINAL_SCALE * 1.15 + (FINAL_SCALE - FINAL_SCALE * 1.15) * e) * baseSize();
					} else {
						ts = FINAL_SCALE * baseSize();
					}
				} else {
					ts = 0; to = 0;
				}
			} else if (m === "chat") {
				if (isSelected && store.presenting) {
					tx = 50;
					ty = 35;
					ts = FINAL_SCALE * baseSize() * 0.8;
				} else if (isSelected) {
					// The chat carries its own small moon under the last message
					// (CompanionPresence); the scene moon shrinks away toward it.
					tx = 50;
					ty = 70;
					ts = HOME_SCALE * baseSize();
					to = 0;
				} else {
					ts = 0; to = 0;
				}
			}

			if (useLerp) {
				orb.x += (tx - orb.x) * lerpF;
				orb.y += (ty - orb.y) * lerpF;
				orb.size += (ts - orb.size) * lerpF;
				orb.opacity += (to - orb.opacity) * lerpF;
			} else {
				orb.x = tx;
				orb.y = ty;
				orb.size = ts;
				orb.opacity = to;
			}

			orb.visible = orb.size > 1 && orb.opacity > 0.01;
		}

		orbs = newOrbs;
		// (all videos are now square — no format compensation needed)


		if (m !== lastMode) {
			lastMode = m;
		}
	}

	onMount(() => {
		// Mobile: animation still runs but orb is blurred/faded via CSS
		prevTime = performance.now();
		animate();
	});

	onDestroy(() => {
		if (raf) cancelAnimationFrame(raf);
	});


</script>

<div class="scene-root" bind:this={container}>
	{#each orbs as orb (orb.slug)}
		{#if orb.visible}
			<button
				class="orb-btn"
				aria-label={orb.slug}
				style="left: {orb.x}%; top: {orb.y}%; width: {orb.size}px; height: {orb.size}px; opacity: {orb.opacity};"
				disabled={store.mode !== "home"}
			>
				<!-- The face and motion follow the companion-state reducer (#86); in chat the scene moon fades out and CompanionPresence carries the face and status. -->
				<MoonExpression kind={store.companion.kind} class="moon-avatar" />
			</button>
		{/if}
	{/each}


</div>

<style>
	.scene-root {
		position: absolute;
		inset: 0;
		z-index: 0;
		overflow: hidden;
		pointer-events: none;
	}


	.orb-btn {
		display:flex;align-items:center;justify-content:center;
		position: absolute;
		background: none;
		border: none;
		padding: 0;
		cursor: pointer;
		pointer-events: auto;
		transform: translate(-50%, -50%);
		will-change: left, top, width, height, opacity;
		overflow: visible;
	}

	.orb-btn:disabled {
		cursor: default;
	}

	.orb-btn :global(.moon-avatar) { width: 70%; height: 70%; max-width:320px; max-height:320px; }

	@media (max-width: 640px) {
		.orb-btn :global(.moon-avatar) { max-width:200px; max-height:200px; }
		.orb-btn { pointer-events: none; }
	}
</style>
