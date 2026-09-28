<script lang="ts">
	import { enhance } from '$app/forms';
	import ServerIcon from '@lucide/svelte/icons/server';
	import type { CustomProviderStatus } from '@nolune/core';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import CustomProviderFields from './CustomProviderFields.svelte';
	import { getI18n } from '$lib/i18n';
	import { submitting, type CustomProviderResult } from './custom-providers';

	/**
	 * A custom provider among the API keys: its name, API, address and whether it has a key (never
	 * the key), with a form to change them. Its API stays: its presets' chats are in it. `presets`:
	 * to say which run on it before it's removed. `result`: the last save's, when it's about this
	 * one.
	 */
	let {
		provider,
		presets,
		result
	}: {
		provider: CustomProviderStatus;
		presets: { name: string; provider: string; model: string }[];
		result: CustomProviderResult | null | undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.customProviders);

	let editing = $state(false);
	let saving = $state(false);
	let removing = $state(false);
	let url = $state('');

	const mine = $derived(result?.customProvider === provider.id ? result : null);
	/** Its presets: a custom model is `<id>/<model>`. */
	const usedBy = $derived(
		presets
			.filter((p) => p.provider.startsWith('custom-') && p.model.startsWith(`${provider.id}/`))
			.map((p) => p.name)
	);

	function open() {
		url = provider.url;
		editing = true;
	}
</script>

<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
		<span
			class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-foreground max-sm:self-start"
		>
			<ServerIcon class="size-4" />
		</span>
		<div class="min-w-0 flex-1">
			<div class="font-medium">{provider.name}</div>
			<div class="text-muted-foreground">{t.about(t.apis[provider.api])}</div>
			<div class="truncate text-muted-foreground">
				<span class="font-mono">{provider.url}</span> ·
				{provider.hasKey ? t.withKey(provider.hint) : t.noKey}
			</div>
		</div>
		<!-- Under the text on phones, so it keeps the width. -->
		<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
			{#if !editing}
				<Button variant="ghost" size="sm" class="text-muted-foreground" onclick={open}>
					{t.change}
				</Button>
			{/if}
			<Button
				variant="ghost"
				size="sm"
				class="text-muted-foreground"
				onclick={() => (removing = true)}
			>
				{m.common.remove}
			</Button>
		</div>
	</div>

	{#if editing}
		<form
			method="POST"
			action="?/saveCustomProvider"
			class="space-y-4 sm:pl-12"
			use:enhance={submitting(
				(value) => (saving = value),
				() => (editing = false)
			)}
		>
			<input type="hidden" name="id" value={provider.id} />
			<CustomProviderFields {provider} bind:url />
			{#if mine?.customError}
				<p class="text-destructive" role="alert">{mine.customError}</p>
			{/if}
			<div class="flex gap-2">
				<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
					{saving ? t.checking : m.common.save}
				</Button>
				<Button type="button" variant="ghost" class="h-10" onclick={() => (editing = false)}>
					{m.common.cancel}
				</Button>
			</div>
		</form>
	{:else if mine?.customWarning}
		<p class="text-warning sm:pl-12" role="status">{mine.customWarning}</p>
	{:else if mine?.customMessage}
		<p class="text-muted-foreground sm:pl-12" role="status">{mine.customMessage}</p>
	{/if}
</li>

<AlertDialog.Root open={removing} onOpenChange={(value) => (removing = value)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{t.removeTitle(provider.name)}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if usedBy.length}
					{t.usedBy(usedBy.join(', '))}
				{/if}
				{t.removeBody}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/removeCustomProvider"
			use:enhance={() => {
				return async ({ update }) => {
					removing = false;
					await update();
				};
			}}
		>
			<input type="hidden" name="id" value={provider.id} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.common.remove}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
