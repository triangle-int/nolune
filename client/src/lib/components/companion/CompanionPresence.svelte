<script lang="ts">
	/**
	 * Little Moon inside the conversation: a small moon under the last
	 * message with the companion's live status beside it, and the memories
	 * the current turn recalled as links. The only moon in the chat; the
	 * face and motion come from the same reducer as everywhere else (#86).
	 */
	import type { RecalledMemory } from "$lib/api/types.js";
	import type { CompanionState } from "$lib/companion/state.js";
	import MoonExpression from "./MoonExpression.svelte";
	import CompanionStatus from "./CompanionStatus.svelte";

	let {
		state,
		name = "",
		slug = null,
		memories = [],
		reducedMotion,
	}: {
		state: CompanionState;
		name?: string;
		slug?: string | null;
		memories?: RecalledMemory[];
		reducedMotion?: boolean;
	} = $props();

	const memoryLabel = (path: string) => path.split("/").pop()?.replace(/\.md$/, "") ?? path;
</script>

<div class="presence" data-kind={state.kind}>
	<MoonExpression kind={state.kind} size={40} {reducedMotion} class="presence-moon" />
	<div class="presence-body">
		<CompanionStatus {state} {name} {slug} class="presence-status" />
		{#if memories.length > 0}
			<ul class="presence-memories" aria-label="Memories recalled for this reply">
				{#each memories as memory, i (memory.path)}
					<li style="animation-delay: {i * 80}ms">
						{#if slug}
							<a href="/{slug}/memory?open={encodeURIComponent(memory.path)}">{memoryLabel(memory.path)}</a>
						{:else}
							<span>{memoryLabel(memory.path)}</span>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>

<style>
	.presence {
		display: flex;
		align-items: flex-start;
		gap: 12px;
		padding: 8px 0 16px;
		min-width: 0;
	}
	.presence :global(.presence-moon) {
		flex-shrink: 0;
		width: 40px;
		height: 40px;
	}
	.presence-body {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-width: 0;
		padding-top: 10px;
	}
	.presence :global(.presence-status) {
		overflow-wrap: anywhere;
	}
	.presence-memories {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.presence-memories li {
		animation: memory-in 0.35s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	.presence-memories a,
	.presence-memories span {
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		padding: 4px 12px;
		border: 1px solid var(--border);
		border-radius: 8px;
		background: var(--card);
		font: 400 12px/1.4 var(--font-body);
		color: var(--primary);
		text-decoration: none;
		white-space: nowrap;
	}
	.presence-memories a:hover {
		border-color: var(--primary);
		background: var(--accent);
	}
	@keyframes memory-in {
		from { opacity: 0; transform: translateY(4px); }
		to { opacity: 1; transform: translateY(0); }
	}
	@media (prefers-reduced-motion: reduce) {
		.presence-memories li { animation: none; }
	}
</style>
