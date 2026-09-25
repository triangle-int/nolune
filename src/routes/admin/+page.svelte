<script lang="ts">
	import { enhance } from '$app/forms';
	import BoxIcon from '@lucide/svelte/icons/box';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import TopBar from '$lib/components/TopBar.svelte';
	import { formatTokens } from '$lib/format';

	let { data, form } = $props();
	let adding = $state(false);
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-8 px-4 py-8 sm:py-12">
			<div class="space-y-1">
				<h1 class="text-2xl font-semibold">Models</h1>
				<p class="text-muted-foreground">
					Presets are shared by every profile. Removing one doesn't affect existing chats.
				</p>
			</div>

			{#if form?.message}
				<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
			{/if}

			<ul class="overflow-hidden rounded-2xl border">
				{#each data.presets as preset (preset.id)}
					<li class="flex items-center gap-3 border-b px-4 py-3 text-sm last:border-b-0">
						<span
							class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground"
						>
							<BoxIcon class="size-4" />
						</span>
						<div class="min-w-0 flex-1">
							<div class="truncate font-medium">{preset.name}</div>
							<div class="truncate text-muted-foreground">
								{preset.provider} / {preset.model} · context {formatTokens(
									preset.contextWindow
								)}{preset.overridden ? ' (override)' : ''}
							</div>
						</div>
						<form method="POST" action="?/remove" use:enhance>
							<input type="hidden" name="id" value={preset.id} />
							<Button type="submit" variant="ghost" size="sm" class="text-muted-foreground"
								>Remove</Button
							>
						</form>
					</li>
				{:else}
					<li class="px-4 py-3 text-sm text-muted-foreground">No presets yet.</li>
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
				<Input
					name="model"
					required
					placeholder="Model id, e.g. claude-opus-5-5"
					aria-label="Model id"
					class="h-10 rounded-full px-4"
				/>
				<div class="flex flex-col gap-3 sm:flex-row">
					<Input
						name="name"
						placeholder="Name (default: model + provider)"
						aria-label="Name"
						class="h-10 flex-1 rounded-full px-4"
					/>
					<Input
						name="contextWindow"
						type="number"
						min="1"
						placeholder="Context window (optional)"
						aria-label="Context window"
						class="h-10 rounded-full px-4 sm:w-60"
					/>
				</div>
				<Button type="submit" disabled={adding} class="h-10 px-5">
					{adding ? 'Checking the model…' : 'Add'}
				</Button>
			</form>
		</div>
	</main>
</div>
