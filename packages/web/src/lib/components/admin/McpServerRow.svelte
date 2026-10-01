<script lang="ts">
	import { enhance } from '$app/forms';
	import PlugIcon from '@lucide/svelte/icons/plug';
	import type { McpServerStatus } from '@nolune/core';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { Button } from '$lib/components/ui/button';
	import McpServerFields from './McpServerFields.svelte';
	import { getI18n } from '$lib/i18n';
	import { submitting } from './custom-providers';
	import type { McpKind, McpServerResult } from './mcp-servers';

	/**
	 * A connected MCP server: its name, what it's for, its command or address, the names of its
	 * keys (never the keys) and the profiles that have it, with buttons to check it, change it and
	 * disconnect it. One whose settings in config.json are broken can only be disconnected.
	 * `result`: the last save's, check's or removal's, when it's about this one.
	 */
	let {
		server,
		profiles,
		result
	}: {
		server: McpServerStatus;
		profiles: { slug: string; name: string }[];
		result: McpServerResult | null | undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.mcp);

	let editing = $state(false);
	let saving = $state(false);
	let checking = $state(false);
	let removing = $state(false);
	let kind = $state<McpKind>('stdio');
	let transport = $state<'http' | 'sse'>('http');
	let chosen = $state<string[]>([]);

	const mine = $derived(result?.mcpServer === server.name ? result : null);
	/** The profiles by name; one deleted since keeps its slug. */
	const where = $derived(
		server.profiles
			? t.onlyIn(server.profiles.map((s) => profiles.find((p) => p.slug === s)?.name ?? s))
			: t.everyProfile
	);

	/** The form starts from what's saved. */
	function open() {
		kind = server.type === 'stdio' ? 'stdio' : 'remote';
		transport = server.type === 'sse' ? 'sse' : 'http';
		chosen = (server.profiles ?? []).filter((s) => profiles.some((p) => p.slug === s));
		editing = true;
	}
</script>

<li class="space-y-3 border-b px-4 py-3 text-sm last:border-b-0">
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
		<span
			class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-foreground max-sm:self-start"
		>
			<PlugIcon class="size-4" />
		</span>
		<div class="min-w-0 flex-1">
			<div class="font-medium">{server.name}</div>
			{#if server.problem}
				<div class="break-words text-warning">{t.broken(server.problem)}</div>
			{:else}
				{#if server.description}
					<div class="text-muted-foreground">{server.description}</div>
				{/if}
				<div class="truncate font-mono text-muted-foreground">{server.target}</div>
				<!-- Apart from the command, which a long one would cut off. -->
				<div class="text-muted-foreground">
					{where} · {server.secrets.length ? t.keys(server.secrets) : t.noKeys}
				</div>
			{/if}
		</div>
		<!-- Under the text on phones, so it keeps the width. -->
		<div class="flex flex-wrap gap-1 max-sm:basis-full max-sm:pl-9">
			{#if !editing && !server.problem}
				<form
					method="POST"
					action="?/checkMcpServer"
					use:enhance={() => {
						checking = true;
						return async ({ update }) => {
							await update({ reset: false });
							checking = false;
						};
					}}
				>
					<input type="hidden" name="name" value={server.name} />
					<Button
						type="submit"
						variant="ghost"
						size="sm"
						class="text-muted-foreground"
						disabled={checking}
					>
						{checking ? t.checking : t.check}
					</Button>
				</form>
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
				{t.disconnect}
			</Button>
		</div>
	</div>

	{#if editing}
		<form
			method="POST"
			action="?/saveMcpServer"
			class="space-y-4 sm:pl-12"
			use:enhance={submitting(
				(value) => (saving = value),
				() => (editing = false)
			)}
		>
			<input type="hidden" name="editing" value={server.name} />
			<McpServerFields {server} {profiles} bind:kind bind:transport bind:chosen />
			{#if mine?.mcpError}
				<p class="break-words text-destructive" role="alert">{mine.mcpError}</p>
			{/if}
			<div class="flex gap-2">
				<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
					{saving ? t.connecting : m.common.save}
				</Button>
				<Button type="button" variant="ghost" class="h-10" onclick={() => (editing = false)}>
					{m.common.cancel}
				</Button>
			</div>
		</form>
	{:else if mine?.mcpError}
		<p class="break-words text-destructive sm:pl-12" role="alert">{mine.mcpError}</p>
	{:else if mine?.mcpWarning}
		<p class="break-words text-warning sm:pl-12" role="status">{mine.mcpWarning}</p>
	{:else if mine?.mcpMessage}
		<p class="break-words text-muted-foreground sm:pl-12" role="status">{mine.mcpMessage}</p>
	{/if}
</li>

<AlertDialog.Root open={removing} onOpenChange={(value) => (removing = value)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{t.removeTitle(server.name)}</AlertDialog.Title>
			<AlertDialog.Description>{t.removeBody}</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action="?/removeMcpServer"
			use:enhance={() => {
				return async ({ update }) => {
					removing = false;
					await update();
				};
			}}
		>
			<input type="hidden" name="name" value={server.name} />
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">{t.disconnect}</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
