<script lang="ts">
	import { enhance } from '$app/forms';

	let { data, form } = $props();
</script>

<main class="mx-auto max-w-lg space-y-8 overflow-y-auto p-6">
	<h1 class="text-xl font-semibold">Profile settings</h1>

	{#if form?.message}
		<p class="rounded-md bg-stone-100 px-3 py-2 text-sm">{form.message}</p>
	{/if}

	<form method="POST" action="?/rename" use:enhance class="space-y-2">
		<h2 class="font-medium">Name</h2>
		<div class="flex gap-2">
			<input
				name="name"
				value={data.profile.name}
				required
				class="flex-1 rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-stone-500 focus:outline-none"
			/>
			<button class="rounded-md border border-stone-300 px-4 py-2 text-sm hover:bg-stone-50"
				>Rename</button
			>
		</div>
		<p class="text-xs text-stone-500">
			Folder: ~/.btw-agent/profiles/{data.profile.slug} (doesn't change)
		</p>
	</form>

	<section class="space-y-2">
		<h2 class="font-medium">Members</h2>
		<ul class="divide-y divide-stone-200 rounded-xl border border-stone-200 bg-white">
			{#each data.members as member (member.id)}
				<li class="flex items-center justify-between px-4 py-2 text-sm">
					<span>{member.name}</span>
					<form method="POST" action="?/remove" use:enhance>
						<input type="hidden" name="userId" value={member.id} />
						<button class="text-stone-500 hover:text-red-600">Remove</button>
					</form>
				</li>
			{/each}
		</ul>
		{#if data.others.length}
			<form method="POST" action="?/add" use:enhance class="flex gap-2">
				<select
					name="who"
					class="flex-1 rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-stone-500 focus:outline-none"
				>
					{#each data.others as name (name)}
						<option value={name}>{name}</option>
					{/each}
				</select>
				<button class="rounded-md border border-stone-300 px-4 py-2 text-sm hover:bg-stone-50"
					>Add</button
				>
			</form>
		{:else}
			<p class="text-sm text-stone-500">Everyone is already a member.</p>
		{/if}
	</section>

	<form
		method="POST"
		action="?/delete"
		class="space-y-2"
		onsubmit={(event) => {
			if (
				!confirm(
					`Delete "${data.profile.name}" and all its conversations? The folder is moved to ~/.btw-agent/trash.`
				)
			) {
				event.preventDefault();
			}
		}}
	>
		<h2 class="font-medium text-red-700">Delete profile</h2>
		<button class="rounded-md border border-red-300 px-4 py-2 text-sm text-red-700 hover:bg-red-50">
			Delete this profile
		</button>
	</form>
</main>
