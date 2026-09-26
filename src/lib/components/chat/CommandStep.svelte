<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import { firstLine } from '$lib/commands';
	import { getPreferences } from '$lib/preferences.svelte';
	import { resultStatus, type ToolResult } from '$lib/transcript';
	import { cn } from '$lib/utils';
	import ViewedImages from './ViewedImages.svelte';

	interface Props {
		conversationId: string;
		command: string | null;
		cwd?: string;
		/** What the model said the command does, in plain words. */
		summary: string | null;
		result: ToolResult | undefined;
		/** Output streamed so far while the command runs. */
		liveOutput: string | null;
		/** The agent is still working, so a command without a result is running, not abandoned. */
		running: boolean;
	}

	let { conversationId, command, cwd, summary, result, liveOutput, running }: Props = $props();

	const prefs = getPreferences();
	let open = $state(false);

	const status = $derived(result ? resultStatus(result) : running ? 'running' : 'not run');
	/** Calls from before the model wrote summaries have none. */
	const label = $derived(
		summary ??
			(command === null
				? 'Getting ready…'
				: status === 'running'
					? 'Running a command'
					: 'Ran a command')
	);
	const output = $derived(result?.output ?? liveOutput ?? '');
	/** The technical label already shows a short command in full. */
	const showCommand = $derived(!prefs.technical || !command || command !== firstLine(command));
</script>

<Collapsible.Root bind:open>
	<Collapsible.Trigger
		class="group/step flex w-full min-w-0 items-center gap-2 rounded-lg py-0.5 text-left text-sm text-muted-foreground hover:text-foreground"
	>
		{#if prefs.technical}
			<span class="min-w-0 truncate font-mono text-xs">
				{command === null ? 'Preparing a command…' : `$ ${firstLine(command)}`}
			</span>
		{:else}
			<span class="min-w-0 truncate">{label}</span>
		{/if}
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
			<div class="border-b px-3 py-1.5 text-muted-foreground">
				{prefs.technical ? (summary ?? 'Command') : 'The command btw ran'}
			</div>
			{#if showCommand}
				<pre
					class="overflow-x-auto border-b px-3 py-2 font-mono break-all whitespace-pre-wrap">{command ??
						'…'}</pre>
			{/if}
			{#if cwd && prefs.technical}
				<div class="border-b px-3 py-1.5 font-mono text-muted-foreground">in {cwd}</div>
			{/if}
			<pre
				class={cn(
					'max-h-72 overflow-auto px-3 py-2 font-mono break-all whitespace-pre-wrap',
					status === 'failed' ? 'text-destructive' : 'text-muted-foreground'
				)}>{output || (status === 'running' ? 'No output yet…' : '(no output)')}</pre>
		</div>
	</Collapsible.Content>
	<!-- What it looked at stays in sight when the command and its output are folded away. -->
	{#if result?.images.length}
		<ViewedImages {conversationId} images={result.images} />
	{/if}
</Collapsible.Root>
