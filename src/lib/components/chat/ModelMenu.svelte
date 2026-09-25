<script lang="ts" module>
	export const EFFORT_INFO: Record<string, { label: string; hint: string }> = {
		low: { label: 'Low', hint: 'Fastest answers' },
		medium: { label: 'Medium', hint: 'Good for most things' },
		high: { label: 'High', hint: 'Thinks longer on harder tasks' },
		xhigh: { label: 'Extra high', hint: 'Takes its time' },
		max: { label: 'Max', hint: 'Slowest, for the hardest problems' }
	};

	/** "claude-opus-5-5 (anthropic)" → "claude-opus-5-5". */
	export function shortModelName(name: string): string {
		return name.replace(/\s*\((anthropic)\)$/i, '');
	}
</script>

<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { getPreferences } from '$lib/preferences.svelte';

	interface Props {
		efforts: string[];
		effort: string;
		onEffortChange: (effort: string) => void;
		/** Choosable models (a new chat), or just the current one (the model is fixed per chat). */
		presets: { id: string; name: string }[];
		presetId: string;
		onPresetChange?: (id: string) => void;
	}

	let { efforts, effort, onEffortChange, presets, presetId, onPresetChange }: Props = $props();

	const prefs = getPreferences();
	const preset = $derived(presets.find((p) => p.id === presetId));
	const locked = $derived(!onPresetChange);
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger
		class="flex h-9 max-w-64 min-w-0 items-center gap-1 rounded-full px-3 text-sm text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground sm:max-w-sm"
	>
		<span class="truncate">
			{#if prefs.technical && preset}
				{shortModelName(preset.name)} ·
			{/if}
			{EFFORT_INFO[effort]?.label ?? effort}
		</span>
		<ChevronDownIcon class="size-3.5 shrink-0" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Content side="top" align="start" class="w-72">
		{#if !locked && presets.length > 1}
			<DropdownMenu.Label class="text-xs font-normal text-muted-foreground"
				>Model</DropdownMenu.Label
			>
			<DropdownMenu.RadioGroup value={presetId} onValueChange={(id) => onPresetChange?.(id)}>
				{#each presets as p (p.id)}
					<DropdownMenu.RadioItem value={p.id}>{shortModelName(p.name)}</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
			<DropdownMenu.Separator />
		{:else if locked && preset && prefs.technical}
			<DropdownMenu.Label class="font-normal">
				<span class="block text-xs text-muted-foreground">Model (fixed for this chat)</span>
				<span class="block truncate">{preset.name}</span>
			</DropdownMenu.Label>
			<DropdownMenu.Separator />
		{/if}
		<DropdownMenu.Label class="text-xs font-normal text-muted-foreground"
			>Reasoning</DropdownMenu.Label
		>
		<DropdownMenu.RadioGroup value={effort} onValueChange={onEffortChange}>
			{#each efforts as level (level)}
				<DropdownMenu.RadioItem value={level} class="items-start">
					<span class="flex flex-col">
						<span>{EFFORT_INFO[level]?.label ?? level}</span>
						<span class="text-xs font-normal text-muted-foreground">
							{EFFORT_INFO[level]?.hint ?? ''}
						</span>
					</span>
				</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
		{#if locked}
			<p class="px-3 pt-1 pb-2 text-xs text-muted-foreground">
				{prefs.technical
					? 'Changing this makes the next reply re-read the whole conversation once (a prompt cache miss).'
					: 'After a change, the next reply takes a little longer.'}
			</p>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
