<script lang="ts">
	import { enhance } from '$app/forms';
	import { AVATARS } from '@btw/core/avatars';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import AssistantAvatar from '$lib/components/AssistantAvatar.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import UserAvatar from '$lib/components/UserAvatar.svelte';
	import { cn } from '$lib/utils';

	let { data, form } = $props();

	let who = $state('');
	let deleteOpen = $state(false);
	/** The avatar just clicked, shown as picked until the page has saved it. */
	let picking = $state<string | null>(null);
	const avatar = $derived(picking ?? data.profile.avatar);
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
				picking = submitter?.getAttribute('value') ?? null;
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
			<div class="grid grid-cols-4 gap-2 sm:grid-cols-8">
				{#each AVATARS as name (name)}
					{@const picked = name === avatar}
					<button
						type="submit"
						name="avatar"
						value={name}
						aria-pressed={picked}
						class={cn(
							'flex flex-col items-center gap-1.5 rounded-2xl border px-1 pt-3 pb-2 text-xs text-muted-foreground hover:bg-muted',
							picked && 'border-foreground/60 bg-muted text-foreground'
						)}
					>
						<AssistantAvatar avatar={name} mood={picked ? 'idle' : undefined} size={36} />
						<span class="capitalize">{name}</span>
					</button>
				{/each}
			</div>
		</form>

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
