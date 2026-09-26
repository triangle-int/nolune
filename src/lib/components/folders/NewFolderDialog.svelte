<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { createFolder, type FolderItem } from '$lib/folders';

	interface Props {
		open: boolean;
		/** The profile the folder is for. */
		slug: string;
		oncreated: (folder: FolderItem) => void;
	}

	let { open = $bindable(), slug, oncreated }: Props = $props();

	let name = $state('');
	let saving = $state(false);
	let problem = $state<string | null>(null);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!name.trim() || saving) return;
		saving = true;
		problem = null;
		try {
			const created = await createFolder(slug, name);
			open = false;
			name = '';
			oncreated(created);
		} catch (err) {
			problem = err instanceof Error ? err.message : String(err);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog.Root bind:open onOpenChange={(isOpen) => !isOpen && (problem = null)}>
	<Dialog.Content>
		<form onsubmit={submit} class="grid gap-5">
			<Dialog.Header>
				<Dialog.Title>New folder</Dialog.Title>
				<Dialog.Description>
					Keep related chats together. Every chat in a folder gets its instructions and files.
				</Dialog.Description>
			</Dialog.Header>
			<Input
				bind:value={name}
				placeholder="Trip to Japan"
				maxlength={80}
				aria-label="Folder name"
				class="h-10 rounded-full px-4"
			/>
			{#if problem}
				<p class="text-sm text-destructive">{problem}</p>
			{/if}
			<Dialog.Footer>
				<Button type="submit" disabled={!name.trim() || saving}>Create folder</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
