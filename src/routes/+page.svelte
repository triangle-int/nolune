<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';

	let { data, form } = $props();
</script>

<main class="mx-auto max-w-2xl space-y-8 p-6">
	<section class="space-y-3">
		<h1 class="text-2xl font-semibold">Profiles</h1>
		{#if data.profiles.length === 0}
			<p class="text-stone-600">
				You're not in any profile yet. Create one below, or ask someone to add you to theirs.
			</p>
		{:else}
			<ul
				class="divide-y divide-stone-200 overflow-hidden rounded-xl border border-stone-200 bg-white"
			>
				{#each data.profiles as profile (profile.slug)}
					<li>
						<a
							href={resolve('/p/[slug]', { slug: profile.slug })}
							class="block px-4 py-3 hover:bg-stone-50"
						>
							<div class="font-medium">{profile.name}</div>
							<div class="text-sm text-stone-500">{profile.members.join(', ')}</div>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<form method="POST" action="?/create" use:enhance class="space-y-2">
		<h2 class="font-medium">New profile</h2>
		<div class="flex gap-2">
			<input
				name="name"
				placeholder="e.g. Family, Grandma, Homework"
				required
				class="flex-1 rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-stone-500 focus:outline-none"
			/>
			<button
				class="rounded-md bg-stone-900 px-4 py-2 text-sm font-medium text-white hover:bg-stone-700"
			>
				Create
			</button>
		</div>
		{#if form?.message}
			<p class="text-sm text-red-600">{form.message}</p>
		{/if}
	</form>
</main>
