<script lang="ts">
	import { enhance } from '$app/forms';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import type { FolderItem } from '$lib/folders';

	interface Props {
		/** The folder to rename; the dialog is open while it's set. */
		folder: FolderItem | null;
		slug: string;
	}

	let { folder = $bindable(), slug }: Props = $props();

	let saving = $state(false);
	let problem = $state<string | null>(null);
</script>

<Dialog.Root
	open={folder !== null}
	onOpenChange={(isOpen) => {
		if (isOpen) return;
		folder = null;
		problem = null;
	}}
>
	<Dialog.Content>
		{#if folder}
			<form
				method="POST"
				action="/p/{slug}/f/{folder.id}?/rename"
				class="grid gap-5"
				use:enhance={() => {
					saving = true;
					problem = null;
					return async ({ result, update }) => {
						saving = false;
						if (result.type === 'failure') {
							problem = String(result.data?.message ?? 'Could not rename the folder.');
							return;
						}
						folder = null;
						await update();
					};
				}}
			>
				<Dialog.Header>
					<Dialog.Title>Rename folder</Dialog.Title>
				</Dialog.Header>
				<Input
					name="name"
					value={folder.name}
					required
					maxlength={80}
					aria-label="Folder name"
					class="h-10 rounded-full px-4"
				/>
				{#if problem}
					<p class="text-sm text-destructive">{problem}</p>
				{/if}
				<Dialog.Footer>
					<Button type="submit" disabled={saving}>Rename</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
