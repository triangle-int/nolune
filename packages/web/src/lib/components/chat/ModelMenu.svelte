<script lang="ts" module>
	/** "claude-opus-5-5 (anthropic)" → "claude-opus-5-5". */
	export function shortModelName(name: string): string {
		return name.replace(
			/\s*\((anthropic|openai|openrouter|custom-openai|custom-anthropic|claude-plan|chatgpt-plan)\)$/i,
			''
		);
	}
</script>

<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import type { Avatar } from '@nolune/core/avatars';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import EffortSlider from './EffortSlider.svelte';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';

	interface Props {
		efforts: string[];
		effort: string;
		onEffortChange: (effort: string) => void;
		/** The models to choose from. */
		presets: { id: string; name: string }[];
		presetId: string;
		onPresetChange: (id: string) => void;
		/** Marked in the list so people can find their way back to it. */
		defaultPresetId?: string;
		/** The profile's assistant, which flies the reasoning slider. */
		avatar: Avatar;
		/** Whether the menu is open; closed by the chat when a change needs asking about first. */
		open?: boolean;
	}

	let {
		efforts,
		effort,
		onEffortChange,
		presets,
		presetId,
		onPresetChange,
		defaultPresetId,
		avatar,
		open = $bindable(false)
	}: Props = $props();

	const prefs = getPreferences();
	const { m } = getI18n();
	/** Labels and hints by reasoning level; levels added later show as they are. */
	const effortInfo: Record<string, { label: string; hint: string } | undefined> = m.model.efforts;
	const preset = $derived(presets.find((p) => p.id === presetId));
</script>

<DropdownMenu.Root bind:open>
	<DropdownMenu.Trigger
		class="flex h-9 max-w-64 min-w-0 items-center gap-1 rounded-full px-3 text-sm text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground sm:max-w-sm"
	>
		<span class="truncate">
			{#if prefs.technical && preset}
				{shortModelName(preset.name)} ·
			{/if}
			{effortInfo[effort]?.label ?? effort}
		</span>
		<ChevronDownIcon class="size-3.5 shrink-0" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Content side="top" align="start" class="w-72">
		{#if presets.length > 1}
			<DropdownMenu.Label class="text-xs font-normal text-muted-foreground"
				>{m.model.model}</DropdownMenu.Label
			>
			<DropdownMenu.RadioGroup value={presetId} onValueChange={onPresetChange}>
				{#each presets as p (p.id)}
					<DropdownMenu.RadioItem value={p.id}>
						<span class="min-w-0 flex-1 truncate">{shortModelName(p.name)}</span>
						{#if p.id === defaultPresetId}
							<span class="text-xs text-muted-foreground">{m.common.default}</span>
						{/if}
					</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
			<DropdownMenu.Separator />
		{/if}
		<DropdownMenu.Label class="text-xs font-normal text-muted-foreground"
			>{m.model.reasoning}</DropdownMenu.Label
		>
		<EffortSlider {efforts} value={effort} onchange={onEffortChange} {avatar} />
	</DropdownMenu.Content>
</DropdownMenu.Root>
