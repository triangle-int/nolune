<script lang="ts">
	import { enhance } from '$app/forms';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ServerIcon from '@lucide/svelte/icons/server';
	import type { ServerStatus } from '@btw/core';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getI18n } from '$lib/i18n';

	/**
	 * The family's own model servers: each with its address and whether it has a key (never the
	 * key), a form to change it, and one to add another. `presets`: to say which run on a server
	 * before it's removed. `result`: the last save's or removal's, for the server it names, or for
	 * the section when it names none.
	 */
	let {
		servers,
		presets,
		result
	}: {
		servers: ServerStatus[];
		presets: { name: string; provider: string; model: string }[];
		result:
			| {
					server?: string;
					serverMessage?: string;
					serverWarning?: string;
					serverError?: string;
			  }
			| null
			| undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.servers);
	const uid = $props.id();

	/** The server whose form is open, `new` for the add form. */
	let editing = $state<string | null>(null);
	let saving = $state(false);
	let removing = $state<ServerStatus | null>(null);
	let url = $state('');

	const mine = (name: string) => result && result.server === name;

	/** The presets on a server: a custom model is `<server>/<model>`. */
	function presetsOn(name: string): string[] {
		return presets
			.filter((p) => p.provider.startsWith('custom-') && p.model.startsWith(`${name}/`))
			.map((p) => p.name);
	}

	function open(server: ServerStatus | null) {
		url = server?.url ?? '';
		editing = server?.name ?? 'new';
	}

	const submit = () => {
		saving = true;
		return async ({
			result: sent,
			update
		}: {
			result: { type: string };
			update: (opts?: { reset?: boolean }) => Promise<void>;
		}) => {
			await update({ reset: false });
			saving = false;
			if (sent.type === 'success') editing = null;
		};
	};
</script>

{#snippet keyField(server: ServerStatus | null)}
	<div class="space-y-2">
		<label for="{uid}-{server?.name ?? 'new'}-key" class="block font-medium">
			{t.key} <span class="font-normal text-muted-foreground">{t.keyOptional}</span>
		</label>
		<Input
			id="{uid}-{server?.name ?? 'new'}-key"
			name="key"
			type="password"
			autocomplete="off"
			spellcheck="false"
			placeholder={server?.hasKey && url.replace(/\/+$/, '') === server.url ? t.keyKept : ''}
			class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
		/>
	</div>
{/snippet}

{#snippet addressField(server: ServerStatus | null)}
	<div class="space-y-2">
		<label for="{uid}-{server?.name ?? 'new'}-url" class="block font-medium">{t.address}</label>
		<Input
			id="{uid}-{server?.name ?? 'new'}-url"
			name="url"
			bind:value={url}
			required
			autocomplete="off"
			spellcheck="false"
			placeholder="http://localhost:11434"
			aria-describedby="{uid}-{server?.name ?? 'new'}-url-hint"
			class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
		/>
		<p id="{uid}-{server?.name ?? 'new'}-url-hint" class="text-muted-foreground">
			{t.addressHint}
		</p>
	</div>
{/snippet}

{#snippet buttons()}
	<div class="flex gap-2">
		<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
			{saving ? t.checking : m.common.save}
		</Button>
		<Button type="button" variant="ghost" class="h-10" onclick={() => (editing = null)}>
			{m.common.cancel}
		</Button>
	</div>
{/snippet}

<section class="space-y-3" aria-labelledby="{uid}-heading">
	<div class="space-y-1">
		<h2 id="{uid}-heading" class="text-lg font-medium">{t.title}</h2>
		<p class="text-muted-foreground">{t.hint}</p>
	</div>

	{#if mine('') && !editing}
		{#if result?.serverWarning}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm text-warning" role="status">
				{result.serverWarning}
			</p>
		{:else if result?.serverMessage}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="status">{result.serverMessage}</p>
		{/if}
	{/if}

	<ul class="overflow-hidden rounded-2xl border">
		{#each servers as server (server.name)}
			<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
				<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
					<span
						class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-foreground max-sm:self-start"
					>
						<ServerIcon class="size-4" />
					</span>
					<div class="min-w-0 flex-1">
						<div class="font-medium">{server.name}</div>
						<div class="truncate font-mono text-muted-foreground">{server.url}</div>
						<div class="text-muted-foreground">
							{server.hasKey ? t.withKey(server.hint) : t.noKey}
						</div>
					</div>
					<div class="flex gap-1 max-sm:basis-full max-sm:pl-9">
						{#if editing !== server.name}
							<Button
								variant="ghost"
								size="sm"
								class="text-muted-foreground"
								onclick={() => open(server)}
							>
								{t.change}
							</Button>
						{/if}
						<Button
							variant="ghost"
							size="sm"
							class="text-muted-foreground"
							onclick={() => (removing = server)}
						>
							{m.common.remove}
						</Button>
					</div>
				</div>

				{#if editing === server.name}
					<form method="POST" action="?/saveServer" class="space-y-4 sm:pl-12" use:enhance={submit}>
						<input type="hidden" name="name" value={server.name} />
						{@render addressField(server)}
						{@render keyField(server)}
						{#if mine(server.name) && result?.serverError}
							<p class="text-destructive" role="alert">{result.serverError}</p>
						{/if}
						{@render buttons()}
					</form>
				{:else if mine(server.name) && result?.serverWarning}
					<p class="text-warning sm:pl-12" role="status">{result.serverWarning}</p>
				{:else if mine(server.name) && result?.serverMessage}
					<p class="text-muted-foreground sm:pl-12" role="status">{result.serverMessage}</p>
				{/if}
			</li>
		{:else}
			<li class="px-4 py-3 text-sm text-muted-foreground">{t.none}</li>
		{/each}
	</ul>

	{#if editing === 'new'}
		<form
			method="POST"
			action="?/saveServer"
			class="space-y-4 rounded-2xl border p-4 text-sm sm:p-5"
			aria-labelledby="{uid}-add"
			use:enhance={submit}
		>
			<h3 id="{uid}-add" class="text-base font-medium">{t.add}</h3>
			<input type="hidden" name="adding" value="1" />
			<div class="space-y-2">
				<label for="{uid}-name" class="block font-medium">{t.name}</label>
				<Input
					id="{uid}-name"
					name="name"
					required
					autocomplete="off"
					spellcheck="false"
					placeholder="local"
					aria-describedby="{uid}-name-hint"
					class="h-10 rounded-full px-4 font-mono placeholder:font-sans sm:w-60"
				/>
				<p id="{uid}-name-hint" class="text-muted-foreground">{t.nameHint}</p>
			</div>
			{@render addressField(null)}
			{@render keyField(null)}
			{#if mine('') && result?.serverError}
				<p class="text-destructive" role="alert">{result.serverError}</p>
			{/if}
			{@render buttons()}
		</form>
	{:else}
		<Button variant="outline" class="h-10 rounded-full px-4" onclick={() => open(null)}>
			<PlusIcon />
			{t.add}
		</Button>
	{/if}
</section>

<AlertDialog.Root open={removing !== null} onOpenChange={(value) => !value && (removing = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{t.removeTitle(removing?.name ?? '')}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if removing && presetsOn(removing.name).length}
					{t.usedBy(presetsOn(removing.name).join(', '))}
				{/if}
				{t.removeBody}
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/removeServer"
			use:enhance={() => {
				return async ({ update }) => {
					removing = null;
					await update();
				};
			}}
		>
			<input type="hidden" name="name" value={removing?.name ?? ''} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.common.remove}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
