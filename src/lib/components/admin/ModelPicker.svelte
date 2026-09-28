<script lang="ts">
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import type { ModelChoice } from '@nolune/core';
	import * as Command from '$lib/components/ui/command';
	import * as Popover from '$lib/components/ui/popover';
	import Rich from '$lib/components/Rich.svelte';
	import { formatTokens } from '$lib/format';
	import { getI18n } from '$lib/i18n';

	interface Props {
		/** The chosen model id: one from the list, or one typed in. */
		value: string;
		/** The provider's models; null while they're being asked for. */
		models: ModelChoice[] | null;
		/** Who is being asked, for "Asking … for its models". */
		source: string;
		id?: string;
		describedBy?: string;
	}

	let { value = $bindable(), models, source, id, describedBy }: Props = $props();

	const t = getI18n().m.admin.addModel;

	let open = $state(false);
	let search = $state('');
	const typed = $derived(search.trim());
	const chosen = $derived(models?.find((m) => m.id === value));
	/** Its id under its name, unless that's only its name after a custom provider's id. */
	const showsId = (model: ModelChoice | undefined) =>
		!!model?.name && !model.id.endsWith(`/${model.name}`);

	function pick(model: string) {
		value = model;
		open = false;
		search = '';
	}
</script>

<Popover.Root bind:open>
	<Popover.Trigger
		{id}
		aria-describedby={describedBy}
		class="flex h-10 w-full min-w-0 items-center gap-2 rounded-full border border-transparent bg-input/50 px-4 text-left text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/30"
	>
		{#if value}
			<span class="truncate">{chosen?.name ?? value}</span>
			{#if showsId(chosen)}
				<span class="truncate font-mono text-xs text-muted-foreground">{value}</span>
			{/if}
		{:else}
			<span class="text-muted-foreground">{t.pick}</span>
		{/if}
		<ChevronsUpDownIcon class="ml-auto size-4 shrink-0 text-muted-foreground" />
	</Popover.Trigger>
	<Popover.Content align="start" class="w-(--bits-popover-anchor-width) min-w-72 gap-0 p-0">
		<Command.Root>
			<Command.Input bind:value={search} placeholder={t.search} />
			<Command.List class="p-1">
				{#if models === null}
					<Command.Loading class="px-3 py-6 text-center text-sm text-muted-foreground">
						{t.asking(source)}
					</Command.Loading>
				{:else}
					{#each models as model (model.id)}
						<Command.Item
							value={model.id}
							keywords={[model.name ?? '', model.description ?? '']}
							data-checked={model.id === value}
							onSelect={() => pick(model.id)}
						>
							<span class="min-w-0 flex-1">
								<span class="block truncate">{model.name ?? model.id}</span>
								{#if showsId(model)}
									<span class="block truncate font-mono text-xs font-normal text-muted-foreground">
										{model.id}
									</span>
								{/if}
							</span>
							{#if model.contextWindow}
								<span class="shrink-0 text-xs font-normal text-muted-foreground">
									{formatTokens(model.contextWindow)}
								</span>
							{/if}
						</Command.Item>
					{/each}
				{/if}
				{#if typed && !models?.some((m) => m.id === typed)}
					<!-- Always there, so any id can be used: a new model, a snapshot, a proxy's own. -->
					<Command.Item value={typed} forceMount onSelect={() => pick(typed)}>
						<span class="min-w-0 truncate">
							<Rich text={t.use}>
								{#snippet model()}<span class="font-mono">{typed}</span>{/snippet}
							</Rich>
						</span>
					</Command.Item>
				{:else if models?.length === 0}
					<p class="px-3 py-6 text-center text-sm text-muted-foreground">
						{t.typeId}
					</p>
				{/if}
			</Command.List>
		</Command.Root>
	</Popover.Content>
</Popover.Root>
