<script lang="ts">
	import { enhance } from '$app/forms';
	import { goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import type { FolderItem } from '$lib/folders';

	interface Props {
		/** The folder to delete; the dialog is open while it's set. */
		folder: FolderItem | null;
		slug: string;
	}

	let { folder = $bindable(), slug }: Props = $props();
</script>

<AlertDialog.Root open={folder !== null} onOpenChange={(isOpen) => !isOpen && (folder = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete folder?</AlertDialog.Title>
			<AlertDialog.Description>
				<strong class="text-foreground">{folder?.name}</strong> is deleted for everyone in the profile.
				Its chats move back to your chat list, without its instructions and files. The files are moved
				to ~/.btw-agent/trash.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form
			method="POST"
			action={folder ? `/p/${slug}/f/${folder.id}?/delete` : ''}
			use:enhance={() => {
				const id = folder?.id;
				return async ({ result }) => {
					folder = null;
					if (result.type !== 'redirect') return;
					// Stay where you are unless the open page was the folder's.
					if (page.params.folder === id) {
						await goto(resolve('/p/[slug]', { slug }), { invalidateAll: true });
					} else await invalidate('btw:conversations');
				};
			}}
		>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Delete</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
