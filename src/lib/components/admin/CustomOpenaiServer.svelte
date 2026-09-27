<script lang="ts">
	import { enhance } from '$app/forms';
	import ServerIcon from '@lucide/svelte/icons/server';
	import type { CustomOpenaiStatus } from '@btw/core';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	/**
	 * The Custom OpenAI server, as a row among the API keys: its address, whether it has a key
	 * (never the key), and a form to set it. `env`: the variable btw reads the address from when
	 * none is saved.
	 */
	let {
		server,
		env,
		result
	}: {
		server: CustomOpenaiStatus;
		env: string;
		result:
			{ customMessage?: string; customWarning?: string; customError?: string } | null | undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.custom);
	const uid = $props.id();

	let open = $state(false);
	let saving = $state(false);
	let removing = $state(false);
	let url = $state('');

	const details = $derived(
		server.url
			? `${server.source === 'env' ? t.fromEnv(env) : t.saved} · ${
					server.hasKey ? t.keyEnding(server.hint) : t.noKey
				}`
			: m.admin.notSet
	);

	/** The form starts from what's saved. */
	function start() {
		url = server.url ?? '';
		open = true;
	}
</script>

<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
		<span
			class={cn(
				'flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted max-sm:self-start',
				server.url ? 'text-foreground' : 'text-muted-foreground'
			)}
		>
			<ServerIcon class="size-4" />
		</span>
		<div class="min-w-0 flex-1">
			<div class="font-medium">{t.name}</div>
			<div class="text-muted-foreground">{t.about}</div>
			{#if server.url}
				<div class="truncate font-mono text-muted-foreground">{server.url}</div>
			{/if}
			<div class="text-muted-foreground">{details}</div>
		</div>
		<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
			{#if !open}
				<Button variant="ghost" size="sm" class="text-muted-foreground" onclick={start}>
					{server.url ? t.change : t.setUp}
				</Button>
			{/if}
			{#if server.source === 'config'}
				<Button
					variant="ghost"
					size="sm"
					class="text-muted-foreground"
					onclick={() => (removing = true)}
				>
					{m.common.remove}
				</Button>
			{/if}
		</div>
	</div>

	{#if open}
		<form
			method="POST"
			action="?/saveCustomOpenai"
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
				<label for="{uid}-url" class="block font-medium">{t.address}</label>
				<Input
					id="{uid}-url"
					name="url"
					bind:value={url}
					required
					autocomplete="off"
					spellcheck="false"
					placeholder="http://localhost:11434/v1"
					aria-describedby="{uid}-hint"
					class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
				/>
				<p id="{uid}-hint" class="text-muted-foreground">{t.hint}</p>
			</div>
			<div class="space-y-2">
				<label for="{uid}-key" class="block font-medium">
					{t.key} <span class="font-normal text-muted-foreground">{t.keyOptional}</span>
				</label>
				<Input
					id="{uid}-key"
					name="key"
					type="password"
					autocomplete="off"
					spellcheck="false"
					placeholder={server.hasKey && url.replace(/\/+$/, '') === server.url ? t.keyKept : ''}
					class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
				/>
			</div>

			{#if result?.customError}
				<p class="text-destructive" role="alert">{result.customError}</p>
			{/if}

			<div class="flex gap-2">
				<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
					{saving ? t.checking : m.common.save}
				</Button>
				<Button type="button" variant="ghost" class="h-10" onclick={() => (open = false)}>
					{m.common.cancel}
				</Button>
			</div>
		</form>
	{:else if result?.customWarning}
		<p class="text-warning sm:pl-12" role="status">{result.customWarning}</p>
	{:else if result?.customMessage}
		<p class="text-muted-foreground sm:pl-12" role="status">{result.customMessage}</p>
	{/if}

	<AlertDialog.Root open={removing} onOpenChange={(value) => !value && (removing = false)}>
		<AlertDialog.Content>
			<AlertDialog.Header>
				<AlertDialog.Title>{t.removeTitle}</AlertDialog.Title>
				<AlertDialog.Description>
					{server.envSet ? t.useEnvInstead(env) : t.removeBody}
				</AlertDialog.Description>
			</AlertDialog.Header>
			<form
				method="POST"
				action="?/removeCustomOpenai"
				use:enhance={() => {
					return async ({ update }) => {
						removing = false;
						await update();
					};
				}}
			>
				<AlertDialog.Footer>
					<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
					<AlertDialog.Action type="submit" variant="destructive"
						>{m.common.remove}</AlertDialog.Action
					>
				</AlertDialog.Footer>
			</form>
		</AlertDialog.Content>
	</AlertDialog.Root>
</li>
