<script lang="ts">
	import { resolve } from '$app/paths';
	import type { DisplayMemoryLook } from '@btw/core';
	import BrainIcon from '@lucide/svelte/icons/brain';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import MemoryChangeItem from '$lib/components/memory/MemoryChangeItem.svelte';
	import { getI18n } from '$lib/i18n';
	import { memoryAnchor } from '$lib/memory';
	import { cn } from '$lib/utils';

	/**
	 * What btw saved to memory by itself once the chat went quiet, after the last message it read:
	 * a folded line, and each change with its note and Undo. Undoing shows here live.
	 */
	let { look, slug }: { look: DisplayMemoryLook; slug: string } = $props();

	const { m } = getI18n();
	const t = $derived(m.memory.changes);
	const allUndone = $derived(look.changes.every((change) => change.undone));
</script>

<Collapsible.Root class="rounded-2xl border px-4 py-2.5 text-sm">
	<Collapsible.Trigger
		class="group/memory flex w-full min-w-0 items-center gap-1.5 text-left text-xs font-medium text-muted-foreground hover:text-foreground"
	>
		<BrainIcon class="size-3.5 shrink-0" />
		<span class={cn('min-w-0 truncate', allUndone && 'line-through')}>
			{t.saved(look.changes.length)}
		</span>
		<ChevronRightIcon
			class="size-3.5 shrink-0 transition-transform group-data-[state=open]/memory:rotate-90"
		/>
	</Collapsible.Trigger>
	<Collapsible.Content>
		<ul class="mt-2.5 space-y-2.5 pb-1">
			{#each look.changes as change (change.id)}
				<MemoryChangeItem {change} {slug}>
					{#snippet note(topic)}
						<a
							href="{resolve('/p/[slug]/memory', { slug })}#{memoryAnchor(change.note)}"
							class="underline-offset-2 hover:text-foreground hover:underline"
							aria-label={t.openNote(topic)}>{topic}</a
						>
					{/snippet}
				</MemoryChangeItem>
			{/each}
		</ul>
	</Collapsible.Content>
</Collapsible.Root>
