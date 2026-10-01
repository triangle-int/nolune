<script lang="ts">
	import type { McpServerStatus } from '@nolune/core';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';
	import { getI18n } from '$lib/i18n';
	import type { McpKind } from './mcp-servers';

	/**
	 * An MCP server's settings, to connect one (`server` null, with its name) or change it: how
	 * nolune reaches it, its command or address, its keys (never shown: left empty, the saved ones
	 * stay), what it's for and the profiles that have it. `kind`, `transport` and `chosen` are
	 * bound, so whoever opens the form starts them from what's saved.
	 */
	let {
		server,
		profiles,
		kind = $bindable(),
		transport = $bindable(),
		chosen = $bindable()
	}: {
		server: McpServerStatus | null;
		profiles: { slug: string; name: string }[];
		kind: McpKind;
		transport: 'http' | 'sse';
		chosen: string[];
	} = $props();

	const { m } = getI18n();
	const t = $derived(m.admin.mcp);
	const uid = $props.id();
	const KINDS: McpKind[] = ['stdio', 'remote'];
	const TRANSPORTS = ['http', 'sse'] as const;

	/** The saved keys' names, while the server stays the kind they belong to. */
	const kept = $derived(
		server && (server.type === 'stdio') === (kind === 'stdio') ? server.secrets : []
	);
</script>

{#if !server}
	<div class="space-y-2">
		<label for="{uid}-name" class="block font-medium">{t.name}</label>
		<Input
			id="{uid}-name"
			name="name"
			required
			maxlength={32}
			autocomplete="off"
			spellcheck="false"
			placeholder={t.namePlaceholder}
			aria-describedby="{uid}-name-hint"
			class="h-10 rounded-full px-4 font-mono placeholder:font-sans sm:w-60"
		/>
		<p id="{uid}-name-hint" class="text-muted-foreground">{t.nameHint}</p>
	</div>
{/if}

<div class="space-y-2">
	<div id="{uid}-kind" class="font-medium">{t.kind}</div>
	<ToggleGroup.Root
		type="single"
		variant="outline"
		size="sm"
		value={kind}
		onValueChange={(value) => {
			if (value) kind = value as McpKind;
		}}
		aria-labelledby="{uid}-kind"
		class="flex-wrap"
	>
		{#each KINDS as id (id)}
			<ToggleGroup.Item value={id}>{t.kinds[id]}</ToggleGroup.Item>
		{/each}
	</ToggleGroup.Root>
	<input type="hidden" name="kind" value={kind} />
</div>

{#if kind === 'stdio'}
	<div class="space-y-2">
		<label for="{uid}-command" class="block font-medium">{t.command}</label>
		<Input
			id="{uid}-command"
			name="command"
			value={server?.type === 'stdio' ? server.target : ''}
			required
			autocomplete="off"
			spellcheck="false"
			placeholder={t.commandPlaceholder}
			aria-describedby="{uid}-command-hint"
			class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
		/>
		<p id="{uid}-command-hint" class="text-muted-foreground">{t.commandHint}</p>
	</div>
{:else}
	<div class="space-y-2">
		<label for="{uid}-url" class="block font-medium">{t.address}</label>
		<Input
			id="{uid}-url"
			name="url"
			type="url"
			value={server && server.type !== 'stdio' ? server.target : ''}
			required
			autocomplete="off"
			spellcheck="false"
			placeholder="https://example.com/mcp"
			aria-describedby="{uid}-url-hint"
			class="h-10 rounded-full px-4 font-mono placeholder:font-sans"
		/>
		<p id="{uid}-url-hint" class="text-muted-foreground">{t.addressHint}</p>
	</div>
	<div class="space-y-2">
		<div id="{uid}-transport" class="font-medium">{t.transport}</div>
		<ToggleGroup.Root
			type="single"
			variant="outline"
			size="sm"
			value={transport}
			onValueChange={(value) => {
				if (value) transport = value as 'http' | 'sse';
			}}
			aria-labelledby="{uid}-transport"
			class="flex-wrap"
		>
			{#each TRANSPORTS as id (id)}
				<ToggleGroup.Item value={id}>{t.transports[id]}</ToggleGroup.Item>
			{/each}
		</ToggleGroup.Root>
		<input type="hidden" name="transport" value={transport} />
	</div>
{/if}

<div class="space-y-2">
	<label for="{uid}-secrets" class="block font-medium">
		{kind === 'stdio' ? t.env : t.headers}
		<span class="font-normal text-muted-foreground">{t.ifNeeded}</span>
	</label>
	<Textarea
		id="{uid}-secrets"
		name="secrets"
		rows={2}
		autocomplete="off"
		spellcheck="false"
		placeholder={kept.length
			? t.secretsKept(kept)
			: kind === 'stdio'
				? 'GITHUB_TOKEN=ghp_…'
				: 'Authorization: Bearer …'}
		aria-describedby="{uid}-secrets-hint"
		class="px-4 font-mono placeholder:font-sans"
	/>
	<p id="{uid}-secrets-hint" class="text-muted-foreground">
		{kind === 'stdio' ? t.envHint : t.headersHint}
	</p>
</div>

<div class="space-y-2">
	<label for="{uid}-description" class="block font-medium">
		{t.description} <span class="font-normal text-muted-foreground">{t.optional}</span>
	</label>
	<Input
		id="{uid}-description"
		name="description"
		value={server?.description ?? ''}
		maxlength={300}
		autocomplete="off"
		placeholder={t.descriptionPlaceholder}
		aria-describedby="{uid}-description-hint"
		class="h-10 rounded-full px-4"
	/>
	<p id="{uid}-description-hint" class="text-muted-foreground">{t.descriptionHint}</p>
</div>

{#if profiles.length}
	<div class="space-y-2">
		<div id="{uid}-profiles" class="font-medium">{t.profiles}</div>
		<ToggleGroup.Root
			type="multiple"
			variant="outline"
			size="sm"
			bind:value={chosen}
			aria-labelledby="{uid}-profiles"
			class="flex-wrap"
		>
			{#each profiles as profile (profile.slug)}
				<ToggleGroup.Item value={profile.slug}>{profile.name}</ToggleGroup.Item>
			{/each}
		</ToggleGroup.Root>
		<p class="text-muted-foreground">{t.profilesHint}</p>
		{#each chosen as slug (slug)}
			<input type="hidden" name="profiles" value={slug} />
		{/each}
	</div>
{/if}
