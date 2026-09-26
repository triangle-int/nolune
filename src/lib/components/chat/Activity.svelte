<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import { firstLine } from '$lib/commands';
	import { getPreferences } from '$lib/preferences.svelte';
	import {
		formatDuration,
		resultStatus,
		type ActivityPart,
		type ToolResult
	} from '$lib/transcript';
	import { memoryStepCommand, memoryStepIcon, memoryStepLabel } from '$lib/memory';
	import { cn } from '$lib/utils';
	import CommandStep from './CommandStep.svelte';
	import Markdown from './Markdown.svelte';
	import MemoryStep from './MemoryStep.svelte';
	import StepIcon from './StepIcon.svelte';

	interface Props {
		part: ActivityPart;
		results: Record<string, ToolResult>;
		toolOutput: { id: string; text: string } | null;
		/** This is the work happening right now. */
		active: boolean;
		/** The agent is running (commands without a result are still to come, not abandoned). */
		running: boolean;
	}

	let { part, results, toolOutput, active, running }: Props = $props();

	const prefs = getPreferences();
	/** Set once the reader opens or closes the group; until then the preference decides. */
	let choice = $state<boolean | null>(null);
	const open = $derived(choice ?? prefs.expandSteps);

	/** Commands and memory calls: the steps that get a result. */
	const calls = $derived(
		part.steps.filter((s): s is Exclude<typeof s, { type: 'thinking' }> => s.type !== 'thinking')
	);
	const commands = $derived(calls.filter((c) => c.type === 'command'));
	const failed = $derived(
		calls.filter((c) => results[c.id] && resultStatus(results[c.id]) === 'failed').length
	);
	const stopped = $derived(
		calls.some((c) => results[c.id] && resultStatus(results[c.id]) === 'stopped')
	);

	const label = $derived.by(() => {
		if (active) {
			const last = part.steps.at(-1);
			if (last?.type === 'command' && !results[last.id]) {
				if (prefs.technical && last.command) return `Running ${firstLine(last.command, 80)}`;
				return last.summary ?? 'Running a command';
			}
			if (last?.type === 'memory' && !results[last.id]) {
				return prefs.technical ? memoryStepCommand(last) : memoryStepLabel(last);
			}
			return 'Thinking';
		}
		if (stopped) return 'Stopped';
		const duration = formatDuration(part.endedAt - part.startedAt);
		if (calls.length === 0) return duration ? `Thought for ${duration}` : 'Thought for a moment';
		if (prefs.technical && commands.length) {
			const ran = `Ran ${commands.length} command${commands.length === 1 ? '' : 's'}`;
			return duration ? `${ran} · ${duration}` : ran;
		}
		return duration ? `Worked for ${duration}` : 'Worked for a moment';
	});
</script>

<Collapsible.Root bind:open={() => open, (value) => (choice = value)}>
	<Collapsible.Trigger
		class="group/activity flex max-w-full min-w-0 items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
	>
		<span
			class={cn(
				'min-w-0 truncate',
				active && 'thinking-shimmer',
				active && prefs.technical && 'font-mono'
			)}
		>
			{label}
		</span>
		{#if failed && !active && prefs.technical}
			<span class="shrink-0 text-destructive">· {failed} failed</span>
		{/if}
		<ChevronRightIcon
			class="size-4 shrink-0 transition-transform group-data-[state=open]/activity:rotate-90"
		/>
	</Collapsible.Trigger>
	<Collapsible.Content>
		<ol
			class="relative mt-3 mb-1 space-y-3 pl-7 before:absolute before:top-2.5 before:bottom-2.5 before:left-[9px] before:w-px before:bg-border"
		>
			{#each part.steps as step, i (step.type === 'thinking' ? `thinking-${i}` : `${step.type}-${step.id}`)}
				<li class="relative min-w-0">
					<span
						class="absolute top-0.5 -left-7 flex size-5 items-center justify-center bg-background text-muted-foreground"
					>
						{#if step.type === 'command'}
							<StepIcon name={step.icon} class="size-3.5" />
						{:else if step.type === 'memory'}
							<StepIcon name={memoryStepIcon(step)} class="size-3.5" />
						{:else}
							<span class="size-1.5 rounded-full bg-muted-foreground/60"></span>
						{/if}
					</span>
					{#if step.type === 'thinking'}
						{#if step.text.trim()}
							<Markdown text={step.text} class="text-sm leading-relaxed text-muted-foreground" />
						{:else}
							<span class="text-sm text-muted-foreground">Thinking…</span>
						{/if}
					{:else if step.type === 'memory'}
						<MemoryStep call={step} result={results[step.id]} {running} />
					{:else}
						<CommandStep
							command={step.command}
							cwd={step.cwd}
							summary={step.summary}
							result={results[step.id]}
							liveOutput={toolOutput?.id === step.id ? toolOutput.text : null}
							{running}
						/>
					{/if}
				</li>
			{/each}
		</ol>
	</Collapsible.Content>
</Collapsible.Root>
