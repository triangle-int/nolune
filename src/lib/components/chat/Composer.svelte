<script lang="ts">
	import type { Snippet } from 'svelte';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import SquareIcon from '@lucide/svelte/icons/square';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { cn } from '$lib/utils';

	interface Props {
		value: string;
		placeholder?: string;
		/** The agent is working: an empty box shows Stop instead of Send. */
		running?: boolean;
		busy?: boolean;
		/** Name of the textarea, when the composer sits in a form that posts it. */
		name?: string;
		autofocus?: boolean;
		textarea?: HTMLTextAreaElement | null;
		onsubmit: () => void;
		onstop?: () => void;
		/** Controls next to the send button, like the model and reasoning menu. */
		tools?: Snippet;
		/** Above the text, e.g. previews of attached pictures. */
		attachments?: Snippet;
		class?: string;
	}

	let {
		value = $bindable(),
		placeholder = 'Ask anything',
		running = false,
		busy = false,
		name,
		autofocus = false,
		textarea = $bindable(null),
		onsubmit,
		onstop,
		tools,
		attachments,
		class: className
	}: Props = $props();

	const empty = $derived(!value.trim());

	/** Grows with the text up to a limit, then scrolls. */
	function autosize(node: HTMLTextAreaElement) {
		void value;
		node.style.height = 'auto';
		node.style.height = `${Math.min(node.scrollHeight, window.innerHeight * 0.4)}px`;
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return;
		// On phones the return key adds a line, like in the ChatGPT app; the button sends.
		if (window.matchMedia('(pointer: coarse)').matches) return;
		event.preventDefault();
		if (!empty && !busy) onsubmit();
	}

	$effect(() => {
		if (autofocus && !window.matchMedia('(pointer: coarse)').matches) textarea?.focus();
	});
</script>

<div
	class={cn(
		'cursor-text rounded-[28px] border bg-composer p-2 shadow-[0_4px_16px_rgb(0_0_0/0.04)] transition-shadow focus-within:shadow-[0_4px_20px_rgb(0_0_0/0.08)] dark:border-transparent dark:shadow-none',
		className
	)}
	role="presentation"
	onclick={(event) => {
		if (event.target === event.currentTarget) textarea?.focus();
	}}
>
	{@render attachments?.()}
	<textarea
		bind:this={textarea}
		bind:value
		{name}
		{placeholder}
		{onkeydown}
		{@attach autosize}
		rows="1"
		enterkeyhint="enter"
		aria-label="Message"
		class="block max-h-[40vh] min-h-11 w-full resize-none bg-transparent px-3 pt-2.5 pb-1 text-base leading-6 outline-none placeholder:text-muted-foreground"
	></textarea>
	<div class="flex items-center gap-1 pt-1">
		{@render tools?.()}
		<div class="flex-1"></div>
		{#if running && empty}
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={onstop}
							class="flex size-9 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-80"
							aria-label="Stop"
						>
							<SquareIcon class="size-3.5 fill-current" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>Stop</Tooltip.Content>
			</Tooltip.Root>
		{:else}
			<button
				type="button"
				onclick={onsubmit}
				disabled={empty || busy}
				class="flex size-9 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-80 disabled:opacity-30"
				aria-label="Send"
			>
				<ArrowUpIcon class="size-5" />
			</button>
		{/if}
	</div>
</div>
