<script lang="ts">
	import { enhance } from '$app/forms';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';

	interface Props {
		/** The chat to rename; the dialog is open while it's set. */
		chat: { id: string; title: string } | null;
		slug: string;
	}

	let { chat = $bindable(), slug }: Props = $props();

	let saving = $state(false);
	let problem = $state<string | null>(null);
	let input = $state<HTMLInputElement | null>(null);
</script>

<Dialog.Root
	open={chat !== null}
	onOpenChange={(isOpen) => {
		if (isOpen) return;
		chat = null;
		problem = null;
	}}
>
	<!-- Opens with the whole title selected, so typing replaces it. -->
	<Dialog.Content
		onOpenAutoFocus={(event) => {
			if (!input) return;
			event.preventDefault();
			input.focus();
			input.select();
		}}
	>
		{#if chat}
			<form
				method="POST"
				action="/p/{slug}/c/{chat.id}?/rename"
				class="grid gap-5"
				use:enhance={() => {
					saving = true;
					problem = null;
					return async ({ result, update }) => {
						saving = false;
						if (result.type === 'failure') {
							problem = String(result.data?.message ?? 'Could not rename the chat.');
							return;
						}
						chat = null;
						await update();
					};
				}}
			>
				<Dialog.Header>
					<Dialog.Title>Rename chat</Dialog.Title>
				</Dialog.Header>
				<Input
					bind:ref={input}
					name="title"
					value={chat.title}
					required
					maxlength={80}
					aria-label="Chat name"
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
