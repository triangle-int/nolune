<script lang="ts">
	import { enhance } from '$app/forms';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { CustomApi } from '@nolune/core';
	import { Button } from '$lib/components/ui/button';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import CustomProviderFields from './CustomProviderFields.svelte';
	import { getI18n } from '$lib/i18n';
	import { submitting, type CustomProviderResult } from './custom-providers';

	/**
	 * Under the API keys: a button that opens the form to add a custom provider, a model server of
	 * the family's own, with its name, API, address and key. `result`: the last save's or
	 * removal's, when it isn't about a row.
	 */
	let { result }: { result: CustomProviderResult | null | undefined } = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.customProviders);
	const uid = $props.id();
	const APIS: CustomApi[] = ['openai', 'anthropic'];

	let open = $state(false);
	let saving = $state(false);
	let api = $state<CustomApi>('openai');
	let url = $state('');

	const mine = $derived(result?.customProvider === '' ? result : null);
</script>

{#if open}
	<form
		method="POST"
		action="?/saveCustomProvider"
		class="space-y-4 rounded-2xl border p-4 text-sm sm:p-5"
		aria-labelledby="{uid}-heading"
		use:enhance={submitting(
			(value) => (saving = value),
			() => (open = false)
		)}
	>
		<div class="space-y-1">
			<h3 id="{uid}-heading" class="text-base font-medium">{t.addTitle}</h3>
			<p class="text-muted-foreground">{t.addHint}</p>
		</div>
		<CustomProviderFields provider={null} bind:url>
			<div class="space-y-2">
				<div id="{uid}-api" class="font-medium">{t.api}</div>
				<ToggleGroup.Root
					type="single"
					variant="outline"
					size="sm"
					value={api}
					onValueChange={(value) => {
						if (value) api = value as CustomApi;
					}}
					aria-labelledby="{uid}-api"
					class="flex-wrap"
				>
					{#each APIS as id (id)}
						<ToggleGroup.Item value={id}>{t.apis[id]}</ToggleGroup.Item>
					{/each}
				</ToggleGroup.Root>
				<p class="text-muted-foreground">{t.apiHints[api]}</p>
				<input type="hidden" name="api" value={api} />
			</div>
		</CustomProviderFields>
		{#if mine?.customError}
			<p class="text-destructive" role="alert">{mine.customError}</p>
		{/if}
		<div class="flex gap-2">
			<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
				{saving ? t.checking : m.common.add}
			</Button>
			<Button type="button" variant="ghost" class="h-10" onclick={() => (open = false)}>
				{m.common.cancel}
			</Button>
		</div>
	</form>
{:else}
	<div class="space-y-3">
		{#if mine?.customMessage}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="status">{mine.customMessage}</p>
		{/if}
		<Button
			variant="outline"
			class="h-10 rounded-full px-4"
			onclick={() => {
				url = '';
				open = true;
			}}
		>
			<PlusIcon />
			{t.add}
		</Button>
	</div>
{/if}
