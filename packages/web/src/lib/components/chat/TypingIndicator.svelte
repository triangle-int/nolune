<script lang="ts">
	import type { Typist } from '@nolune/core';
	import { getI18n } from '$lib/i18n';
	import UserAvatar from '../UserAvatar.svelte';

	interface Props {
		/** The others writing in the chat, in the order they started. */
		typists: Typist[];
	}

	let { typists }: Props = $props();

	const { m } = getI18n();
</script>

<!-- Where their message will land, like one from them: who, over a bubble of bouncing dots. -->
<div class="flex flex-col items-end gap-1" role="status">
	<div class="flex max-w-full items-center gap-1.5 px-1 text-xs text-muted-foreground">
		<span class="flex shrink-0 -space-x-1">
			{#each typists as typist (typist.id)}
				<UserAvatar name={typist.name} class="size-4 text-[9px] ring-2 ring-background" />
			{/each}
		</span>
		<span class="truncate">{m.chat.typing(typists.map((typist) => typist.name))}</span>
	</div>
	<div class="flex h-11 items-center gap-1 rounded-[22px] bg-bubble px-4" aria-hidden="true">
		{#each [0, 1, 2] as i (i)}
			<span class="dot size-2 rounded-full bg-muted-foreground" style:animation-delay="{i * 0.15}s"
			></span>
		{/each}
	</div>
</div>

<style>
	.dot {
		animation: bounce 1.2s ease-in-out infinite;
	}

	@keyframes bounce {
		0%,
		60%,
		100% {
			transform: translateY(0);
			opacity: 0.45;
		}
		30% {
			transform: translateY(-4px);
			opacity: 1;
		}
	}

	@keyframes glow {
		0%,
		60%,
		100% {
			opacity: 0.45;
		}
		30% {
			opacity: 1;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.dot {
			animation-name: glow;
		}
	}
</style>
