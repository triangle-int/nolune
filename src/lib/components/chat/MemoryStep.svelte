<script lang="ts">
	import type { MemoryCall } from '@btw/core';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import { memoryAnchor, memoryStepCommand, memoryStepLabel } from '$lib/memory';
	import { getPreferences } from '$lib/preferences.svelte';
	import { resultStatus, type ToolResult } from '$lib/transcript';
	import { cn } from '$lib/utils';

	interface Props {
		call: MemoryCall;
		result: ToolResult | undefined;
		/** The agent is still working, so a call without a result is running, not abandoned. */
		running: boolean;
	}

	let { call, result, running }: Props = $props();

	const prefs = getPreferences();
	let open = $state(false);

	const status = $derived(result ? resultStatus(result) : running ? 'running' : 'not run');
	/** Views show what was read; changes show what was written. */
	const isChange = $derived(call.command !== 'view');
	/** Only the lines that changed: old_str usually repeats a line or two around the edit. */
	const change = $derived.by(() => {
		const before = call.oldText === undefined ? [] : call.oldText.split('\n');
		const after = call.text === undefined ? [] : call.text.split('\n');
		let start = 0;
		while (start < before.length && start < after.length && before[start] === after[start]) start++;
		let end = 0;
		while (
			end < before.length - start &&
			end < after.length - start &&
			before[before.length - 1 - end] === after[after.length - 1 - end]
		)
			end++;
		const removed = before.slice(start, before.length - end).join('\n');
		const added = after.slice(start, after.length - end).join('\n');
		return { removed: removed || null, added: added || null };
	});
	/** resolve() plus a #fragment for the note's card. */
	const memoryHref = $derived.by(() => {
		const slug = page.params.slug;
		if (!slug) return null;
		const href = resolve('/p/[slug]/memory', { slug });
		const file = call.command === 'delete' ? null : (call.newPath ?? call.path);
		return file && /\.[^/]+$/.test(file) ? `${href}#${memoryAnchor(file)}` : href;
	});
</script>

<Collapsible.Root bind:open>
	<Collapsible.Trigger
		class="group/step flex w-full min-w-0 items-center gap-2 rounded-lg py-0.5 text-left text-sm text-muted-foreground hover:text-foreground"
	>
		<span class={cn('min-w-0 truncate', prefs.technical && 'font-mono text-xs')}>
			{prefs.technical ? memoryStepCommand(call) : memoryStepLabel(call)}
		</span>
		{#if status === 'running'}
			<LoaderIcon class="size-3.5 shrink-0 animate-spin" />
		{:else if status === 'failed'}
			<span class="shrink-0 text-xs text-destructive">
				{prefs.technical ? 'failed' : "didn't work"}
			</span>
		{:else if status === 'stopped' || status === 'not run'}
			<span class="shrink-0 text-xs">{status}</span>
		{/if}
		<ChevronRightIcon
			class="size-3.5 shrink-0 opacity-0 transition-transform group-hover/step:opacity-100 group-data-[state=open]/step:rotate-90 group-data-[state=open]/step:opacity-100"
		/>
	</Collapsible.Trigger>
	<Collapsible.Content>
		<div class="mt-2 mb-1 overflow-hidden rounded-xl border bg-muted/40 text-xs">
			<div class="flex items-center gap-2 border-b px-3 py-1.5 text-muted-foreground">
				<span class="min-w-0 flex-1 truncate">
					{prefs.technical
						? memoryStepLabel(call)
						: call.command === 'view'
							? 'What btw read'
							: 'What btw changed'}
				</span>
				{#if memoryHref}
					<!-- eslint-disable svelte/no-navigation-without-resolve -- memoryHref is resolve() plus a #fragment -->
					<a
						href={memoryHref}
						class="shrink-0 underline-offset-2 hover:text-foreground hover:underline">Open memory</a
					>
					<!-- eslint-enable svelte/no-navigation-without-resolve -->
				{/if}
			</div>
			{#if isChange && change.removed}
				<pre
					class="max-h-40 overflow-auto border-b bg-destructive/5 px-3 py-2 font-sans break-words whitespace-pre-wrap text-muted-foreground line-through decoration-destructive/40">{change.removed}</pre>
			{/if}
			{#if isChange && change.added}
				<pre
					class="max-h-72 overflow-auto border-b px-3 py-2 font-sans break-words whitespace-pre-wrap">{change.added}</pre>
			{/if}
			{#if result && (!isChange || result.isError || prefs.technical || !(change.removed || change.added))}
				<pre
					class={cn(
						'max-h-72 overflow-auto px-3 py-2 font-mono break-all whitespace-pre-wrap',
						result.isError ? 'text-destructive' : 'text-muted-foreground'
					)}>{result.output}</pre>
			{:else if !result}
				<p class="px-3 py-2 text-muted-foreground">
					{status === 'running' ? 'Working on it…' : 'This never ran.'}
				</p>
			{/if}
		</div>
	</Collapsible.Content>
</Collapsible.Root>
