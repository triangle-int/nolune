<script lang="ts">
	import { enhance } from '$app/forms';
	import ListCollapseIcon from '@lucide/svelte/icons/list-collapse';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	/**
	 * After how many quiet minutes the model summarizes a chat (compaction.ts in core), or off.
	 * `setting.minutes` is null while it's off; `setting.max` is the most it takes.
	 */
	let {
		setting,
		result
	}: {
		setting: { minutes: number | null; max: number };
		result: { idleMessage?: string; idleError?: string } | null | undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.idleCompaction);
	const uid = $props.id();
	const MODES = ['off', 'on'] as const;
	/** What the form offers when it's off: just under the hour the prompt cache lasts. */
	const SUGGESTED = 55;

	let open = $state(false);
	let saving = $state(false);
	let mode = $state<(typeof MODES)[number]>('off');
	let minutes = $state(String(SUGGESTED));

	/** The form starts from what's saved. */
	function start() {
		mode = setting.minutes ? 'on' : 'off';
		minutes = String(setting.minutes ?? SUGGESTED);
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
					setting.minutes ? 'text-foreground' : 'text-muted-foreground'
				)}
			>
				<ListCollapseIcon class="size-4" />
			</span>
			<div class="min-w-0 flex-1">
				<div class="font-medium">{t.modes[setting.minutes ? 'on' : 'off']}</div>
				<div class="break-words text-muted-foreground">
					{setting.minutes ? t.onStatus(setting.minutes) : t.offStatus}
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
				action="?/idleCompaction"
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
							if (value) mode = value as typeof mode;
						}}
						aria-labelledby="{uid}-mode"
						class="flex-wrap"
					>
						{#each MODES as id (id)}
							<ToggleGroup.Item value={id}>{t.modes[id]}</ToggleGroup.Item>
						{/each}
					</ToggleGroup.Root>
					<input type="hidden" name="enabled" value={mode} />
				</div>

				{#if mode === 'on'}
					<div class="space-y-2">
						<label for="{uid}-minutes" class="block font-medium">{t.minutes}</label>
						<Input
							id="{uid}-minutes"
							name="minutes"
							type="number"
							inputmode="numeric"
							min="1"
							max={setting.max}
							step="1"
							required
							bind:value={minutes}
							class="h-10 w-32"
						/>
						<p class="text-muted-foreground">{t.cacheNote}</p>
					</div>
				{/if}

				{#if result?.idleError}
					<p class="text-destructive" role="alert">{result.idleError}</p>
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
		{:else if result?.idleMessage}
			<p class="text-muted-foreground sm:pl-12" role="status">{result.idleMessage}</p>
		{/if}
	</div>
</section>
