<script lang="ts">
	import { enhance } from '$app/forms';
	import { isAvatar, type Avatar } from '@btw/core/avatars';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as Select from '$lib/components/ui/select';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import AvatarPicker from '$lib/components/AvatarPicker.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import UserAvatar from '$lib/components/UserAvatar.svelte';
	import { getPreferences } from '$lib/preferences.svelte';

	let { data, form } = $props();

	const prefs = getPreferences();
	let who = $state('');
	let deleteOpen = $state(false);
	/** The avatar just clicked, shown as picked until the page has saved it. */
	let picking = $state<Avatar | null>(null);
	const avatar = $derived(picking ?? data.profile.avatar);

	/** What's in the box; follows the saved soul until someone types. */
	let soul = $derived(data.soul);
	const edited = $derived(soul.trim() !== data.soul);
	let savingSoul = $state(false);
	let soulProblem = $state<string | null>(null);

	/**
	 * Grows with the text up to its max height, then scrolls. Not with `field-sizing: content`
	 * (the Textarea's default): Safari then lays the placeholder out wider than the box.
	 */
	function growWithText(node: HTMLTextAreaElement) {
		void soul;
		node.style.minHeight = '';
		const border = node.offsetHeight - node.clientHeight;
		const max = parseFloat(getComputedStyle(node).maxHeight) || Infinity;
		node.style.minHeight = `${Math.min(node.scrollHeight + border, max)}px`;
	}
</script>

<PageHeader>
	<span class="truncate text-lg font-medium">People & profile</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-xl space-y-10 px-4 py-6 sm:py-10">
		{#if form?.message}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		<form method="POST" action="?/rename" use:enhance class="space-y-3">
			<h2 class="font-medium">Name</h2>
			<div class="flex gap-2">
				<Input
					name="name"
					value={data.profile.name}
					required
					aria-label="Profile name"
					class="h-10 flex-1 rounded-full px-4"
				/>
				<Button type="submit" variant="outline" class="h-10 px-5">Rename</Button>
			</div>
			<p class="text-xs text-muted-foreground">
				Folder: ~/.btw-agent/profiles/{data.profile.slug} (doesn't change)
			</p>
		</form>

		<form
			method="POST"
			action="?/avatar"
			use:enhance={({ submitter }) => {
				const value = submitter?.getAttribute('value');
				picking = isAvatar(value) ? value : null;
				return async ({ update }) => {
					await update({ reset: false });
					picking = null;
				};
			}}
			class="space-y-3"
		>
			<div class="space-y-1">
				<h2 class="font-medium">Avatar</h2>
				<p class="text-sm text-muted-foreground">
					How btw looks in this profile's chats. Everyone here sees the same one, and btw can change
					it when asked.
				</p>
			</div>
			<AvatarPicker {avatar} />
		</form>

		<section class="space-y-3">
			<div class="space-y-1">
				<h2 class="font-medium">Soul</h2>
				<p class="text-sm text-muted-foreground">
					Who btw is for {data.profile.name}: its character, what it cares about, how it talks.
					Every chat starts with it, and btw changes it too when you ask it to be different.
				</p>
			</div>
			<form
				method="POST"
				action="?/soul"
				use:enhance={() => {
					savingSoul = true;
					soulProblem = null;
					return async ({ result, update }) => {
						savingSoul = false;
						if (result.type === 'failure') {
							soulProblem = String(result.data?.message ?? 'Could not save.');
						} else await update({ reset: false });
					};
				}}
			>
				<Textarea
					name="soul"
					bind:value={soul}
					{@attach growWithText}
					maxlength={data.maxSoul}
					aria-label="Soul"
					placeholder="You're warm and a little playful, and you keep answers short. With the kids you explain things simply and never talk down to them. When you don't know something, you say so."
					class="field-sizing-fixed! max-h-96 min-h-32"
				/>
				{#if soulProblem}
					<p class="mt-2 text-sm text-destructive">{soulProblem}</p>
				{/if}
				<div class="mt-2 flex min-h-8 items-center gap-2">
					<span class="text-xs text-muted-foreground tabular-nums">
						{soul.length} / {data.maxSoul} characters
					</span>
					{#if edited}
						<Button variant="ghost" size="sm" class="ml-auto" onclick={() => (soul = data.soul)}
							>Cancel</Button
						>
						<Button type="submit" size="sm" disabled={savingSoul}>Save</Button>
					{/if}
				</div>
			</form>
			<p class="text-xs text-muted-foreground">
				{prefs.technical
					? 'Chats get changes at their next message, which re-reads the conversation once (a prompt cache miss).'
					: 'Chats get changes at their next message.'}
			</p>
		</section>

		<section class="space-y-3">
			<div class="space-y-1">
				<h2 class="font-medium">Members</h2>
				<p class="text-sm text-muted-foreground">
					Everyone here sees and writes in the same chats, and can ask btw for anything.
				</p>
			</div>
			<ul class="overflow-hidden rounded-2xl border">
				{#each data.members as member (member.id)}
					<li class="flex items-center gap-3 border-b px-4 py-2.5 text-sm last:border-b-0">
						<UserAvatar name={member.name} />
						<span class="min-w-0 flex-1 truncate">{member.name}</span>
						<form method="POST" action="?/remove" use:enhance>
							<input type="hidden" name="userId" value={member.id} />
							<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground"
								>Remove</Button
							>
						</form>
					</li>
				{/each}
			</ul>
			{#if data.others.length}
				<form method="POST" action="?/add" use:enhance class="flex gap-2">
					<input type="hidden" name="who" value={who} />
					<Select.Root type="single" bind:value={who}>
						<Select.Trigger class="h-10 flex-1 rounded-full px-4" aria-label="Person to add">
							{who || 'Choose someone to add'}
						</Select.Trigger>
						<Select.Content>
							{#each data.others as name (name)}
								<Select.Item value={name}>{name}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
					<Button type="submit" variant="outline" class="h-10 px-5" disabled={!who}>Add</Button>
				</form>
			{:else}
				<p class="text-sm text-muted-foreground">Everyone is already a member.</p>
			{/if}
		</section>

		<section class="space-y-3">
			<h2 class="font-medium text-destructive">Delete profile</h2>
			<p class="text-sm text-muted-foreground">
				Deletes all its chats for everyone. The folder is moved to ~/.btw-agent/trash.
			</p>
			<Button variant="destructive" onclick={() => (deleteOpen = true)}>Delete this profile</Button>
		</section>
	</div>
</div>

<AlertDialog.Root bind:open={deleteOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete "{data.profile.name}"?</AlertDialog.Title>
			<AlertDialog.Description>
				All its chats are deleted for everyone. The folder is moved to ~/.btw-agent/trash.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form method="POST" action="?/delete">
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Delete</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
