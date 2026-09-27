<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';
	import {
		activeStepLabel,
		formatDuration,
		resultStatus,
		type ActivityPart,
		type ToolResult
	} from '$lib/transcript';
	import { cn } from '$lib/utils';
	import CommandStep from './CommandStep.svelte';
	import Markdown from './Markdown.svelte';
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
	const { m } = getI18n();
	/** Set once the reader opens or closes the group; until then the preference decides. */
	let choice = $state<boolean | null>(null);
	const open = $derived(choice ?? prefs.expandSteps);

	const commands = $derived(
		part.steps.filter((s): s is Extract<typeof s, { type: 'command' }> => s.type === 'command')
	);
	const failed = $derived(
		commands.filter((c) => results[c.id] && resultStatus(results[c.id]) === 'failed').length
	);
	const stopped = $derived(
		commands.some((c) => results[c.id] && resultStatus(results[c.id]) === 'stopped')
	);

	const label = $derived.by(() => {
		if (active) return activeStepLabel(part, results, prefs.technical, m);
		if (stopped) return m.steps.stopped;
		const duration = formatDuration(part.endedAt - part.startedAt, m);
		if (commands.length === 0) {
			return duration ? m.steps.thoughtFor(duration) : m.steps.thoughtForAMoment;
		}
		if (prefs.technical) {
			const ran = m.steps.ranCommands(commands.length);
			return duration ? `${ran} · ${duration}` : ran;
		}
		return duration ? m.steps.workedFor(duration) : m.steps.workedForAMoment;
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
			<span class="shrink-0 text-destructive">· {m.steps.failedCount(failed)}</span>
		{/if}
		<ChevronRightIcon
			class="size-4 shrink-0 transition-transform group-data-[state=open]/activity:rotate-90"
		/>
	</Collapsible.Trigger>
	<Collapsible.Content>
		<ol
			class="relative mt-3 mb-1 space-y-3 pl-7 before:absolute before:top-2.5 before:bottom-2.5 before:left-[9px] before:w-px before:bg-border"
		>
			{#each part.steps as step, i (step.type === 'command' ? `command-${step.id}` : `thinking-${i}`)}
				<li class="relative min-w-0">
					<span
						class="absolute top-0.5 -left-7 flex size-5 items-center justify-center bg-background text-muted-foreground"
					>
						{#if step.type === 'command'}
							<StepIcon name={step.icon} class="size-3.5" />
						{:else}
							<span class="size-1.5 rounded-full bg-muted-foreground/60"></span>
						{/if}
					</span>
					{#if step.type === 'thinking'}
						{#if step.text.trim()}
							<Markdown text={step.text} class="text-sm leading-relaxed text-muted-foreground" />
						{:else}
							<span class="text-sm text-muted-foreground">{m.steps.thinkingDots}</span>
						{/if}
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
