<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { copyText } from '$lib/clipboard';

	let { text, label = 'Copy' }: { text: string; label?: string } = $props();

	let copied = $state(false);

	async function copy() {
		await copyText(text);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}
</script>

<Tooltip.Root>
	<Tooltip.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				onclick={copy}
				class="flex size-8 items-center justify-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground"
				aria-label={copied ? 'Copied' : label}
			>
				{#if copied}
					<CheckIcon class="size-4" />
				{:else}
					<CopyIcon class="size-4" />
				{/if}
			</button>
		{/snippet}
	</Tooltip.Trigger>
	<Tooltip.Content>{copied ? 'Copied' : label}</Tooltip.Content>
</Tooltip.Root>
