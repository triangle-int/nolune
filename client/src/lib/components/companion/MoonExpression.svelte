<script lang="ts">
	/**
	 * Little Moon with the face and motion of a companion state (#86). The
	 * face is one of the static SVGs under `static/skins/moon`; the motion is
	 * a CSS animation chosen by the shared expression table. Under
	 * `prefers-reduced-motion` (or with `reducedMotion` set, as the gallery
	 * does) the face stays and the moon holds still; the status text beside
	 * it carries the meaning. Decorative: the accessible status is
	 * `CompanionStatus.svelte`.
	 */
	import { companionExpression } from "$lib/companion/expressions.js";

	let {
		kind,
		reducedMotion,
		size,
		class: className = "",
	}: { kind: string; reducedMotion?: boolean; size?: number; class?: string } = $props();

	let systemReducedMotion = $state(false);
	$effect(() => {
		if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
		const query = window.matchMedia("(prefers-reduced-motion: reduce)");
		systemReducedMotion = query.matches;
		const update = (event: MediaQueryListEvent) => (systemReducedMotion = event.matches);
		query.addEventListener("change", update);
		return () => query.removeEventListener("change", update);
	});

	const look = $derived(companionExpression(kind, { reducedMotion: reducedMotion ?? systemReducedMotion }));
</script>

<img
	class="moon-expression {className}"
	src={look.asset}
	alt=""
	width={size}
	height={size}
	data-kind={look.kind}
	data-expression={look.expression}
	data-motion={look.motion}
	draggable="false"
/>

<style>
	.moon-expression {
		display: block;
		object-fit: contain;
		pointer-events: none;
		transform-origin: 50% 55%;
		transition: opacity 0.4s ease;
	}
	/* No connection: the face is there, dimmed, and says so in the status text. */
	.moon-expression[data-expression="offline"] {
		opacity: 0.55;
	}
	.moon-expression[data-motion="breathe"] {
		animation: moon-breathe 5s ease-in-out infinite;
	}
	.moon-expression[data-motion="float"] {
		animation: moon-float 3s ease-in-out infinite;
	}
	.moon-expression[data-motion="drift"] {
		animation: moon-drift 4s ease-in-out infinite;
	}
	.moon-expression[data-motion="nod"] {
		animation: moon-nod 1.2s ease-in-out infinite;
	}
	.moon-expression[data-motion="hold"] {
		animation: moon-hold 2.4s ease-in-out infinite;
	}
	.moon-expression[data-motion="settle"] {
		animation: moon-settle 0.7s cubic-bezier(0.34, 1.56, 0.64, 1) 1;
	}
	@keyframes moon-breathe {
		0%, 100% { transform: scale(1); }
		50% { transform: scale(1.02); }
	}
	@keyframes moon-float {
		0%, 100% { transform: translateY(0); }
		50% { transform: translateY(-3%); }
	}
	@keyframes moon-drift {
		0%, 100% { transform: translateY(0) rotate(-2deg); }
		50% { transform: translateY(-3%) rotate(2deg); }
	}
	@keyframes moon-nod {
		0%, 100% { transform: translateY(0); }
		50% { transform: translateY(2%); }
	}
	@keyframes moon-hold {
		0%, 100% { opacity: 1; }
		50% { opacity: 0.8; }
	}
	@keyframes moon-settle {
		0% { transform: scale(1); }
		40% { transform: scale(1.06); }
		100% { transform: scale(1); }
	}
	@media (prefers-reduced-motion: reduce) {
		.moon-expression {
			animation: none;
			transition: none;
		}
	}
</style>
