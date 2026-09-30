<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import AssistantAvatar from '$lib/components/AssistantAvatar.svelte';
	import TopBar from '$lib/components/TopBar.svelte';
	import { getI18n } from '$lib/i18n';

	let { data, form } = $props();
	const { m } = getI18n();
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-xl space-y-10 px-4 py-8 sm:py-12">
			<section class="space-y-4">
				<div class="space-y-1">
					<h1 class="text-2xl font-semibold">{m.profiles.title}</h1>
					<p class="text-muted-foreground">
						{m.profiles.intro}
					</p>
				</div>
				{#if data.profiles.length === 0}
					<p class="rounded-2xl bg-muted px-4 py-3 text-sm">
						{m.profiles.none}
					</p>
				{:else}
					<ul class="overflow-hidden rounded-2xl border">
						{#each data.profiles as profile (profile.slug)}
							<li class="border-b last:border-b-0">
								<a
									href={resolve('/p/[slug]', { slug: profile.slug })}
									class="flex items-center gap-3 px-4 py-3 hover:bg-muted"
								>
									<AssistantAvatar avatar={profile.avatar} size={36} />
									<span class="min-w-0 flex-1">
										<span class="block truncate font-medium">{profile.name}</span>
										<span class="block truncate text-sm text-muted-foreground">
											{profile.members.join(', ')}
										</span>
									</span>
									<ChevronRightIcon class="size-4 text-muted-foreground" />
								</a>
							</li>
						{/each}
					</ul>
				{/if}
			</section>

			<form id="new" method="POST" action="?/create" use:enhance class="space-y-3">
				<h2 class="font-medium">{m.profiles.new}</h2>
				<div class="flex gap-2">
					<Input
						name="name"
						placeholder={m.profiles.namePlaceholder}
						aria-label={m.profiles.name}
						required
						class="h-10 flex-1 rounded-full px-4"
					/>
					<Button type="submit" class="h-10 px-5">{m.common.create}</Button>
				</div>
				{#if form?.message}
					<p class="text-sm text-destructive">{form.message}</p>
				{/if}
			</form>
		</div>
	</main>
</div>
