<script lang="ts">
	import type { Snippet } from 'svelte';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import FileIcon from '@lucide/svelte/icons/file';
	import PaperclipIcon from '@lucide/svelte/icons/paperclip';
	import SquareIcon from '@lucide/svelte/icons/square';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { formatBytes } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import type { Attachments } from '$lib/uploads.svelte';
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
		/** Files attached to the message. Without it, nothing can be attached. */
		attachments?: Attachments;
		class?: string;
	}

	const i18n = getI18n();
	const { m } = i18n;

	let {
		value = $bindable(),
		placeholder = m.composer.placeholder,
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

	let fileInput = $state<HTMLInputElement>();
	let dragging = $state(false);

	const empty = $derived(!value.trim() && !attachments?.ids.length);
	/** Files still uploading hold the message back. */
	const blocked = $derived(busy || !!attachments?.uploading);

	function hasFiles(event: DragEvent): boolean {
		return !!attachments && !!event.dataTransfer?.types.includes('Files');
	}

	function onpaste(event: ClipboardEvent) {
		const files = event.clipboardData?.files;
		if (!attachments || !files?.length) return;
		event.preventDefault();
		attachments.add(files);
	}

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
		if (!empty && !blocked) onsubmit();
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
	class:ring-2={dragging}
	class:ring-ring={dragging}
	role="presentation"
	onclick={(event) => {
		if (event.target === event.currentTarget) textarea?.focus();
	}}
	ondragover={(event) => {
		if (!hasFiles(event)) return;
		event.preventDefault();
		dragging = true;
	}}
	ondragleave={(event) => {
		if (!event.currentTarget.contains(event.relatedTarget as Node | null)) dragging = false;
	}}
	ondrop={(event) => {
		if (!hasFiles(event)) return;
		event.preventDefault();
		dragging = false;
		if (event.dataTransfer?.files.length) attachments?.add(event.dataTransfer.files);
	}}
>
	{#if attachments?.files.length}
		<div class="flex flex-wrap gap-2 px-1 pt-1 pb-1">
			{#each attachments.files as file (file.key)}
				<div
					class={cn(
						'relative flex h-14 max-w-60 items-center gap-2.5 overflow-hidden rounded-xl border bg-background',
						file.preview ? 'w-14' : 'pr-8 pl-2.5',
						file.status === 'failed' && 'border-destructive/50'
					)}
					title={file.error ? `${file.name}: ${file.error}` : file.name}
				>
					{#if file.preview}
						<img src={file.preview} alt={file.name} class="size-full object-cover" />
					{:else}
						<FileIcon class="size-5 shrink-0 text-muted-foreground" />
						<span class="min-w-0 text-sm leading-tight">
							<span class="block truncate font-medium">{file.name}</span>
							<span
								class={cn(
									'block truncate text-xs',
									file.status === 'failed' ? 'text-destructive' : 'text-muted-foreground'
								)}
							>
								{file.status === 'failed' ? file.error : formatBytes(file.size, i18n)}
							</span>
						</span>
					{/if}
					{#if file.status === 'uploading'}
						<div class="absolute inset-x-0 bottom-0 h-1 bg-muted">
							<div
								class="h-full bg-primary transition-[width]"
								style:width="{Math.round(file.progress * 100)}%"
							></div>
						</div>
					{:else if file.status === 'failed' && file.preview}
						<div class="absolute inset-0 bg-destructive/40"></div>
					{/if}
					<button
						type="button"
						onclick={() => attachments.remove(file.key)}
						class="absolute top-1 right-1 flex size-5 items-center justify-center rounded-full bg-foreground/70 text-background hover:bg-foreground"
						aria-label={m.common.removeFile(file.name)}
					>
						<XIcon class="size-3" />
					</button>
				</div>
			{/each}
		</div>
	{/if}
	<textarea
		bind:this={textarea}
		bind:value
		{name}
		{placeholder}
		{onkeydown}
		{onpaste}
		{@attach autosize}
		rows="1"
		enterkeyhint="enter"
		aria-label={m.composer.message}
		class="block max-h-[40vh] min-h-11 w-full resize-none bg-transparent px-3 pt-2.5 pb-1 text-base leading-6 outline-none placeholder:text-muted-foreground"
	></textarea>
	<div class="flex items-center gap-1 pt-1">
		{#if attachments}
			<input
				bind:this={fileInput}
				type="file"
				multiple
				hidden
				onchange={(event) => {
					const input = event.currentTarget;
					if (input.files?.length) attachments.add(input.files);
					input.value = '';
				}}
			/>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={() => fileInput?.click()}
							class="flex size-9 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground"
							aria-label={m.composer.attach}
						>
							<PaperclipIcon class="size-[18px]" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>{m.composer.attach}</Tooltip.Content>
			</Tooltip.Root>
		{/if}
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
							aria-label={m.common.stop}
						>
							<SquareIcon class="size-3.5 fill-current" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>{m.common.stop}</Tooltip.Content>
			</Tooltip.Root>
		{:else}
			<button
				type="button"
				onclick={onsubmit}
				disabled={empty || blocked}
				class="flex size-9 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-80 disabled:opacity-30"
				aria-label={m.composer.send}
			>
				<ArrowUpIcon class="size-5" />
			</button>
		{/if}
	</div>
</div>
