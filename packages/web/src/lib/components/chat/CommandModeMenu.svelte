<script lang="ts">
	import ShieldAlertIcon from '@lucide/svelte/icons/shield-alert';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import type { CommandMode } from '@nolune/core';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		/** How the chat's commands run. */
		mode: CommandMode;
		/** What Models & keys says, marked as the default. */
		fallback: CommandMode;
		/** Admins can turn the check off for a chat; anyone can turn it back on. */
		canUnrestrict: boolean;
		onchange: (mode: CommandMode) => void;
	}

	let { mode, fallback, canUnrestrict, onchange }: Props = $props();

	const { m } = getI18n();
	const t = $derived(m.commandMode);
	/** Unrestricted is out of reach unless it's what the chat would have anyway. */
	const locked = $derived(!canUnrestrict && fallback !== 'unrestricted');
</script>

<!-- A chip in the composer: a shield while commands are checked, a warning when they aren't. -->
<DropdownMenu.Root>
	<DropdownMenu.Trigger
		class={cn(
			'flex h-9 min-w-0 items-center gap-1.5 rounded-full text-sm',
			mode === 'unrestricted'
				? 'bg-warning/10 px-3 text-warning hover:bg-warning/15 aria-expanded:bg-warning/15'
				: 'w-9 justify-center text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground'
		)}
		aria-label={mode === 'auto' ? t.labelAuto : t.labelUnrestricted}
		title={mode === 'auto' ? t.labelAuto : undefined}
	>
		{#if mode === 'auto'}
			<ShieldCheckIcon class="size-[18px] shrink-0" />
		{:else}
			<ShieldAlertIcon class="size-[18px] shrink-0" />
			<span class="truncate max-sm:sr-only">{t.unrestricted}</span>
		{/if}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content side="top" align="start" class="w-72">
		<DropdownMenu.Label class="text-xs font-normal text-muted-foreground"
			>{t.title}</DropdownMenu.Label
		>
		<DropdownMenu.RadioGroup value={mode} onValueChange={(value) => onchange(value as CommandMode)}>
			<DropdownMenu.RadioItem value="auto" class="items-start">
				<ShieldCheckIcon class="mt-0.5 text-muted-foreground" />
				<span class="min-w-0 flex-1">
					<span class="flex items-center gap-2">
						{t.auto}
						{#if fallback === 'auto'}
							<span class="text-xs font-normal text-muted-foreground">{m.common.default}</span>
						{/if}
					</span>
					<span class="block text-xs font-normal text-muted-foreground">{t.autoHint}</span>
				</span>
			</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="unrestricted" disabled={locked} class="items-start">
				<ShieldAlertIcon class="mt-0.5 text-warning" />
				<span class="min-w-0 flex-1">
					<span class="flex items-center gap-2">
						{t.unrestricted}
						{#if fallback === 'unrestricted'}
							<span class="text-xs font-normal text-muted-foreground">{m.common.default}</span>
						{/if}
					</span>
					<span class="block text-xs font-normal text-muted-foreground">{t.unrestrictedHint}</span>
				</span>
			</DropdownMenu.RadioItem>
		</DropdownMenu.RadioGroup>
		{#if locked}
			<p class="px-3 pt-1 pb-2 text-xs text-muted-foreground">{t.adminsOnly}</p>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
