<script lang="ts">
	/**
	 * Browser overlay: Little Moon in a corner, showing what the companion is
	 * really doing (#86). The root layout already feeds every websocket
	 * event through the companion-state reducer, so this page only reads
	 * the scene store: the same face, motion and status text as the chat.
	 */
	import { page } from "$app/state";
	import { getSceneStore } from "$lib/stores/scene.svelte.js";
	import { companionExpression } from "$lib/companion/expressions.js";
	import MoonExpression from "$lib/components/companion/MoonExpression.svelte";
	import CompanionStatus from "$lib/components/companion/CompanionStatus.svelte";

	const slug = $derived(page.params.slug!);
	const scene = getSceneStore();
	// The pip breathes only while the companion is at rest or listening; every
	// other state is carried by the moon's own expression and motion.
	const restful = $derived(companionExpression(scene.companion.kind).motion === "breathe");
</script>

<div class="overlay">
	<div class="pip" class:pip-restful={restful} data-kind={scene.companion.kind}>
		<MoonExpression kind={scene.companion.kind} class="pip-moon" />
	</div>
	<CompanionStatus class="overlay-status" state={scene.companion} name={scene.companionName} {slug} />
</div>

<style>
	:global(html), :global(body) {
		background: transparent !important;
		margin: 0;
		padding: 0;
		overflow: hidden;
	}

	.overlay {
		position: fixed;
		inset: 0;
		pointer-events: none;
	}

	.pip {
		position: absolute;
		bottom: 16px;
		right: 16px;
		width: 56px;
		height: 56px;
		border-radius: 50%;
		overflow: hidden;
		background: var(--card);
		border: 2px solid var(--primary);
		box-shadow: none;
	}
	.pip[data-kind="blocked"], .pip[data-kind="failed"] { border-color: var(--destructive); }
	.pip[data-kind="offline"] { border-color: var(--border); }

	.pip-restful {
		animation: breathe 4s ease-in-out infinite;
	}

	@keyframes breathe {
		0%, 100% { transform: scale(1); }
		50% { transform: scale(1.03); }
	}

	.pip :global(.pip-moon) {
		width: 100%;
		height: 100%;
	}

	/* The words, beside the pip, for a screen reader and a reduced-motion viewer alike. */
	.overlay :global(.overlay-status) {
		position: absolute;
		right: 84px;
		bottom: 24px;
		max-width: calc(100vw - 116px);
		padding: 6px 12px;
		border-radius: var(--radius-control, 8px);
		background: var(--card);
		border: 1px solid var(--border);
		pointer-events: auto;
	}

	@media (prefers-reduced-motion: reduce) {
		.pip-restful { animation: none; }
	}
</style>
