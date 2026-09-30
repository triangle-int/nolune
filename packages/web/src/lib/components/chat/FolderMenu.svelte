<script lang="ts">
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import XIcon from '@lucide/svelte/icons/x';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import NewFolderDialog from '$lib/components/folders/NewFolderDialog.svelte';
	import type { FolderItem } from '$lib/folders';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		folders: FolderItem[];
		/** The folder a new chat starts in, or null. */
		folderId: string | null;
		onchange: (folderId: string | null) => void;
		/** The profile, for making a new folder. */
		slug: string;
	}

	let { folders, folderId, onchange, slug }: Props = $props();

	const { m } = getI18n();
	const folder = $derived(folders.find((f) => f.id === folderId));
	let creating = $state(false);
</script>

<!-- A chip in a new chat's composer: the folder the chat starts in, and a menu to pick one. -->
<div class="relative flex min-w-0 items-center">
	<DropdownMenu.Root>
		<DropdownMenu.Trigger
			class={cn(
				'flex h-9 min-w-0 items-center gap-1.5 rounded-full text-sm',
				folder
					? 'max-w-40 bg-muted pr-8 pl-3 text-foreground hover:bg-accent sm:max-w-60'
					: 'w-9 justify-center text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground'
			)}
			aria-label={folder ? m.folders.inFolder(folder.name) : m.folders.startInFolder}
			title={folder ? undefined : m.folders.startInFolder}
		>
			<FolderIcon class="size-[18px] shrink-0" />
			{#if folder}
				<span class="truncate">{folder.name}</span>
			{/if}
		</DropdownMenu.Trigger>
		<DropdownMenu.Content side="top" align="start" class="w-64">
			<DropdownMenu.Label class="text-xs font-normal text-muted-foreground">
				{m.folders.startInFolder}
			</DropdownMenu.Label>
			{#if folders.length}
				<DropdownMenu.RadioGroup
					value={folderId ?? ''}
					onValueChange={(value) => onchange(value || null)}
				>
					<DropdownMenu.RadioItem value="">{m.folders.noFolder}</DropdownMenu.RadioItem>
					{#each folders as f (f.id)}
						<DropdownMenu.RadioItem value={f.id}>
							<FolderIcon class="text-muted-foreground" />
							<span class="min-w-0 flex-1 truncate">{f.name}</span>
						</DropdownMenu.RadioItem>
					{/each}
				</DropdownMenu.RadioGroup>
				<DropdownMenu.Separator />
			{/if}
			<DropdownMenu.Item onSelect={() => (creating = true)}>
				<FolderPlusIcon />
				{m.folders.newFolderDots}
			</DropdownMenu.Item>
		</DropdownMenu.Content>
	</DropdownMenu.Root>
	{#if folder}
		<button
			type="button"
			onclick={() => onchange(null)}
			class="absolute top-1/2 right-1.5 flex size-6 -translate-y-1/2 items-center justify-center rounded-full text-muted-foreground hover:bg-background hover:text-foreground"
			aria-label={m.folders.startOutside}
		>
			<XIcon class="size-3.5" />
		</button>
	{/if}
</div>

<NewFolderDialog bind:open={creating} {slug} oncreated={(created) => onchange(created.id)} />
