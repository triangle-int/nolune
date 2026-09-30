<script lang="ts">
	import FolderIcon from '@lucide/svelte/icons/folder';
	import FolderInputIcon from '@lucide/svelte/icons/folder-input';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import type { FolderItem } from '$lib/folders';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';

	interface Props {
		folders: FolderItem[];
		/** The folder the chat is in now. */
		folderId: string | null;
		onmove: (folderId: string | null) => void;
		/** "New folder…": make one and move the chat into it. */
		onnew: () => void;
	}

	let { folders, folderId, onmove, onnew }: Props = $props();

	const prefs = getPreferences();
	const { m } = getI18n();
</script>

<!-- A submenu of a chat's menu (sidebar or chat header). -->
<DropdownMenu.Sub>
	<DropdownMenu.SubTrigger>
		<FolderInputIcon />
		{m.folders.moveTo}
	</DropdownMenu.SubTrigger>
	<DropdownMenu.SubContent class="w-60">
		<DropdownMenu.RadioGroup
			value={folderId ?? ''}
			onValueChange={(value) => {
				if (value !== (folderId ?? '')) onmove(value || null);
			}}
		>
			<DropdownMenu.RadioItem value="">{m.folders.noFolder}</DropdownMenu.RadioItem>
			{#each folders as folder (folder.id)}
				<DropdownMenu.RadioItem value={folder.id}>
					<FolderIcon class="text-muted-foreground" />
					<span class="min-w-0 flex-1 truncate">{folder.name}</span>
				</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={onnew}>
			<FolderPlusIcon />
			{m.folders.newFolderDots}
		</DropdownMenu.Item>
		<p class="px-3 pt-1 pb-2 text-xs text-muted-foreground">
			{prefs.technical ? m.folders.moveTechnical : m.folders.move}
		</p>
	</DropdownMenu.SubContent>
</DropdownMenu.Sub>
