<script lang="ts">
	import { enhance } from '$app/forms';
	import { formatTokens } from '$lib/format';

	let { data, form } = $props();
	let adding = $state(false);
</script>

<main class="mx-auto max-w-2xl space-y-8 p-6">
	<h1 class="text-2xl font-semibold">Models</h1>
	<p class="text-sm text-stone-600">
		Presets are shared by every profile. Removing one doesn't affect existing conversations.
	</p>

	{#if form?.message}
		<p class="rounded-md bg-stone-100 px-3 py-2 text-sm">{form.message}</p>
	{/if}

	<ul class="divide-y divide-stone-200 rounded-xl border border-stone-200 bg-white">
		{#each data.presets as preset (preset.id)}
			<li class="flex items-center gap-4 px-4 py-3 text-sm">
				<div class="min-w-0 flex-1">
					<div class="font-medium">{preset.name}</div>
					<div class="text-stone-500">
						{preset.provider} / {preset.model} · context {formatTokens(
							preset.contextWindow
						)}{preset.overridden ? ' (override)' : ''}
					</div>
				</div>
				<form method="POST" action="?/remove" use:enhance>
					<input type="hidden" name="id" value={preset.id} />
					<button class="text-stone-500 hover:text-red-600">Remove</button>
				</form>
			</li>
		{:else}
			<li class="px-4 py-3 text-sm text-stone-500">No presets yet.</li>
		{/each}
	</ul>

	<form
		method="POST"
		action="?/add"
		class="space-y-3"
		use:enhance={() => {
			adding = true;
			return async ({ update }) => {
				await update();
				adding = false;
			};
		}}
	>
		<h2 class="font-medium">Add a preset (Anthropic)</h2>
		<input
			name="model"
			required
			placeholder="Model id, e.g. claude-opus-5-5"
			class="w-full rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-stone-500 focus:outline-none"
		/>
		<div class="flex gap-2">
			<input
				name="name"
				placeholder="Name (default: model + provider)"
				class="flex-1 rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-stone-500 focus:outline-none"
			/>
			<input
				name="contextWindow"
				type="number"
				min="1"
				placeholder="Context window (optional)"
				class="w-56 rounded-md border border-stone-300 bg-white px-3 py-2 text-sm focus:border-stone-500 focus:outline-none"
			/>
		</div>
		<button
			disabled={adding}
			class="rounded-md bg-stone-900 px-4 py-2 text-sm font-medium text-white hover:bg-stone-700 disabled:opacity-50"
		>
			{adding ? 'Checking the model…' : 'Add'}
		</button>
	</form>
</main>
