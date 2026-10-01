<script lang="ts">
	import { enhance } from '$app/forms';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { McpServerStatus } from '@nolune/core';
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import McpServerFields from './McpServerFields.svelte';
	import McpServerRow from './McpServerRow.svelte';
	import { getI18n } from '$lib/i18n';
	import { submitting } from './custom-providers';
	import type { McpKind, McpServerResult } from './mcp-servers';

	/**
	 * The Connected services page's servers: the MCP servers whose tools the agent uses through
	 * `nolune mcp`, each a row, and a form to connect another, open from the start while there are
	 * none. `profiles`: those a server can be kept to. `result`: the last save's, check's or
	 * removal's.
	 */
	let {
		servers,
		profiles,
		result
	}: {
		servers: McpServerStatus[];
		profiles: { slug: string; name: string }[];
		result: McpServerResult | null | undefined;
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.services);
	const uid = $props.id();

	let open = $state(untrack(() => servers.length === 0));
	let saving = $state(false);
	let kind = $state<McpKind>('stdio');
	let transport = $state<'http' | 'sse'>('http');
	let chosen = $state<string[]>([]);

	const mine = $derived(result?.mcpServer === '' ? result : null);

	function start() {
		kind = 'stdio';
		transport = 'http';
		chosen = [];
		open = true;
	}
</script>

<div class="space-y-3">
	{#if servers.length}
		<ul class="overflow-hidden rounded-2xl border">
			{#each servers as server (server.name)}
				<McpServerRow {server} {profiles} {result} />
			{/each}
		</ul>
	{/if}

	{#if open}
		<form
			method="POST"
			action="?/save"
			class="space-y-4 rounded-2xl border p-4 text-sm sm:p-5"
			aria-labelledby="{uid}-add-heading"
			use:enhance={submitting(
				(value) => (saving = value),
				() => (open = false)
			)}
		>
			<div class="space-y-1">
				<h3 id="{uid}-add-heading" class="text-base font-medium">{t.addTitle}</h3>
				<p class="text-muted-foreground">{t.addHint}</p>
			</div>
			<McpServerFields server={null} {profiles} bind:kind bind:transport bind:chosen />
			{#if mine?.mcpError}
				<p class="break-words text-destructive" role="alert">{mine.mcpError}</p>
			{/if}
			<div class="flex gap-2">
				<Button type="submit" disabled={saving} class="h-10 px-5 max-sm:flex-1">
					{saving ? t.connecting : t.add}
				</Button>
				<Button type="button" variant="ghost" class="h-10" onclick={() => (open = false)}>
					{m.common.cancel}
				</Button>
			</div>
		</form>
	{:else}
		<div class="space-y-3">
			{#if mine?.mcpMessage}
				<p class="rounded-2xl bg-muted px-4 py-3 text-sm" role="status">{mine.mcpMessage}</p>
			{/if}
			<Button variant="outline" class="h-10 rounded-full px-4" onclick={start}>
				<PlusIcon />
				{t.add}
			</Button>
		</div>
	{/if}
</div>
