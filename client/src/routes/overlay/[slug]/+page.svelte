<script lang="ts">
	/**
	 * Browser overlay: Little Moon in a corner, showing what the companion is
	 * really doing (#86). The root layout already feeds every websocket
	 * event through the companion-state reducer, so this page reads the
	 * scene store: the same face, motion and status text as the chat. What
	 * the socket cannot carry, the persisted `agent_running` of the default
	 * conversation, it loads itself on every connection, as ChatView does.
	 */
	import { untrack } from "svelte";
	import { page } from "$app/state";
	import { fetchMessages } from "$lib/api/client.js";
	import { getSceneStore } from "$lib/stores/scene.svelte.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import { companionExpression } from "$lib/companion/expressions.js";
	import MoonExpression from "$lib/components/companion/MoonExpression.svelte";
	import CompanionStatus from "$lib/components/companion/CompanionStatus.svelte";

	const slug = $derived(page.params.slug!);
	const scene = getSceneStore();
	const ws = getWebSocket();

	// Persisted state, read once per connection (mount and every reconnect):
	// an overlay opened mid-run shows the run before the next live event, and
	// a run that ended while away is over without being claimed. The socket
	// sends nothing on connect, and only the conversation endpoint knows.
	$effect(() => {
		const connected = ws.connected;
		const currentSlug = slug;
		untrack(() => {
			if (!connected) return;
			fetchMessages(currentSlug, "default")
				.then((res) => {
					if (currentSlug !== slug) return;
					scene.companionEvent({ type: "snapshot", chatId: "default", running: res.agent_running });
				})
				.catch(() => {}); // the overlay still follows the live events
		});
	});
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
