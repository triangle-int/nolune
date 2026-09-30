<script lang="ts">
	import { enhance } from '$app/forms';
	import ShieldAlertIcon from '@lucide/svelte/icons/shield-alert';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import type { CommandMode, CommandSafetyState } from '@nolune/core';
	import { Button } from '$lib/components/ui/button';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	/**
	 * How the agent's commands run: auto mode, where a model checks each one first, or unrestricted.
	 * `presets`: what can do the checks besides each chat's own model.
	 */
	let {
		setting,
		presets,
		result
	}: {
		setting: CommandSafetyState;
		presets: { id: string; name: string }[];
		result: { commandsMessage?: string } | null | undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.commands);
	const uid = $props.id();
	const MODES = ['auto', 'unrestricted'] as const;
	/** Each chat's own model, as a chip. */
	const CHAT = 'chat';

	let open = $state(false);
	let saving = $state(false);
	let mode = $state<CommandMode>('auto');
	/** A preset's id, or CHAT. */
	let checker = $state(CHAT);

	/** What the row says is in use. */
	const status = $derived.by(() => {
		if (setting.mode === 'unrestricted') return { warn: true, text: t.unrestrictedStatus };
		if (setting.presetGone) return { warn: true, text: t.presetGone };
		return {
			warn: false,
			text: setting.preset ? t.checkedBy(setting.preset.name) : t.checkedByChat
		};
	});

	/** The form starts from what's saved. */
	function start() {
		mode = setting.mode;
		checker = setting.preset?.id ?? CHAT;
		open = true;
	}
</script>

<section class="space-y-3" aria-labelledby="{uid}-heading">
	<div class="space-y-1">
		<h2 id="{uid}-heading" class="text-lg font-medium">{t.title}</h2>
		<p class="text-muted-foreground">{t.hint}</p>
	</div>
	<div class="space-y-3 rounded-2xl border px-4 py-3 text-sm">
		<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
			<span
				class={cn(
					'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
					setting.mode === 'auto' ? 'text-foreground' : 'text-warning'
				)}
			>
				{#if setting.mode === 'auto'}
					<ShieldCheckIcon class="size-4" />
				{:else}
					<ShieldAlertIcon class="size-4" />
				{/if}
			</span>
			<div class="min-w-0 flex-1">
				<div class="font-medium">{t.modes[setting.mode]}</div>
				<div class={cn('break-words', status.warn ? 'text-warning' : 'text-muted-foreground')}>
					{status.text}
				</div>
			</div>
			{#if !open}
				<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
					<Button variant="ghost" size="sm" class="text-muted-foreground" onclick={start}>
						{t.change}
					</Button>
				</div>
			{/if}
		</div>

		{#if open}
			<form
				method="POST"
				action="?/commandSafety"
				class="space-y-4 sm:pl-12"
				use:enhance={() => {
					saving = true;
					return async ({ result: sent, update }) => {
						await update({ reset: false });
						saving = false;
						if (sent.type === 'success') open = false;
					};
				}}
			>
				<div class="space-y-2">
					<span id="{uid}-mode" class="block font-medium">{t.mode}</span>
					<ToggleGroup.Root
						type="single"
						variant="outline"
						size="sm"
						value={mode}
						onValueChange={(value) => {
							if (value) mode = value as CommandMode;
						}}
						aria-labelledby="{uid}-mode"
						class="flex-wrap"
					>
						{#each MODES as id (id)}
							<ToggleGroup.Item value={id}>{t.choices[id]}</ToggleGroup.Item>
						{/each}
					</ToggleGroup.Root>
					<p class={mode === 'unrestricted' ? 'text-warning' : 'text-muted-foreground'}>
						{mode === 'auto' ? t.autoNote : t.unrestrictedNote}
					</p>
					<input type="hidden" name="mode" value={mode} />
				</div>

				{#if mode === 'auto'}
					<div class="space-y-2">
						<span id="{uid}-checker" class="block font-medium">{t.checker}</span>
						<ToggleGroup.Root
							type="single"
							variant="outline"
							size="sm"
							value={checker}
							onValueChange={(value) => {
								if (value) checker = value;
							}}
							aria-labelledby="{uid}-checker"
							class="flex-wrap"
						>
							<ToggleGroup.Item value={CHAT}>{t.chatModel}</ToggleGroup.Item>
							{#each presets as preset (preset.id)}
								<ToggleGroup.Item value={preset.id}>{preset.name}</ToggleGroup.Item>
							{/each}
						</ToggleGroup.Root>
						<p class="text-muted-foreground">{t.checkerNote}</p>
						<input type="hidden" name="presetId" value={checker === CHAT ? '' : checker} />
					</div>
				{/if}

				<div class="flex gap-2">
					<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
						{m.common.save}
					</Button>
					<Button type="button" variant="ghost" class="h-10" onclick={() => (open = false)}>
						{m.common.cancel}
					</Button>
				</div>
			</form>
		{:else if result?.commandsMessage}
			<p class="text-muted-foreground sm:pl-12" role="status">{result.commandsMessage}</p>
		{/if}
	</div>
</section>
