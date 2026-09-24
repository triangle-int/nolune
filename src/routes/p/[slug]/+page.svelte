<script lang="ts">
	import { enhance } from '$app/forms';

	let { data, form } = $props();
</script>

<main class="mx-auto max-w-lg space-y-6 p-6">
	<h1 class="text-xl font-semibold">New conversation</h1>
	{#if data.presets.length === 0}
		<p class="text-stone-600">
			No models are set up yet. An admin can add one on the Models page or with
			<code class="rounded bg-stone-100 px-1">btw preset add &lt;model&gt;</code>.
		</p>
	{:else}
		<form method="POST" use:enhance class="space-y-4">
			<label class="block space-y-1 text-sm">
				<span class="text-stone-600">Model</span>
				<select
					name="preset"
					class="w-full rounded-md border border-stone-300 bg-white px-3 py-2 focus:border-stone-500 focus:outline-none"
				>
					{#each data.presets as preset (preset.id)}
						<option value={preset.id}>{preset.name}</option>
					{/each}
				</select>
				<span class="text-xs text-stone-500"
					>The model can't be changed later in this conversation.</span
				>
			</label>
			<label class="block space-y-1 text-sm">
				<span class="text-stone-600">Reasoning</span>
				<select
					name="effort"
					value="medium"
					class="w-full rounded-md border border-stone-300 bg-white px-3 py-2 focus:border-stone-500 focus:outline-none"
				>
					{#each data.efforts as effort (effort)}
						<option value={effort}>{effort}</option>
					{/each}
				</select>
			</label>
			{#if form?.message}
				<p class="text-sm text-red-600">{form.message}</p>
			{/if}
			<button
				class="rounded-md bg-stone-900 px-4 py-2 text-sm font-medium text-white hover:bg-stone-700"
			>
				Start
			</button>
		</form>
	{/if}
</main>
