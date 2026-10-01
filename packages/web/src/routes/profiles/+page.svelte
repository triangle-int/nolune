<!--
	The profiles, and where someone starts: how profiles work, a profile of their own until they
	have one, and shared ones by example. See DESIGN.md, "Web UI".
-->
<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { SubmitFunction } from '@sveltejs/kit';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { AVATARS, type Avatar } from '@nolune/core/avatars';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import AssistantAvatar from '$lib/components/AssistantAvatar.svelte';
	import Rich from '$lib/components/Rich.svelte';
	import TopBar from '$lib/components/TopBar.svelte';
	import UserAvatar from '$lib/components/UserAvatar.svelte';
	import { getI18n, type Messages } from '$lib/i18n';

	let { data, form } = $props();
	const { m } = getI18n();

	type Example = keyof Messages['profiles']['shared']['examples'];

	/** Who else might be in each: dots in the avatars' colors, after the person's own picture. */
	const EXAMPLES: { key: Example; others: Avatar[] }[] = [
		{ key: 'family', others: ['campfire', 'planet', 'comet'] },
		{ key: 'friends', others: ['quantum', 'lantern', 'moon'] },
		{ key: 'couple', others: ['satellite'] }
	];

	const name = $derived(data.user?.name ?? '');
	const firstName = $derived(name.split(/\s+/)[0] ?? '');
	/** In no profile yet: the page is where they start. */
	const first = $derived(data.profiles.length === 0);

	/** Examples they already have a profile by the name of are left out. */
	const examples = $derived.by(() => {
		const taken = new Set(data.profiles.map((p) => p.name.trim().toLowerCase()));
		return EXAMPLES.map((e) => ({ ...e, ...m.profiles.shared.examples[e.key] })).filter(
			(e) => !taken.has(e.name.toLowerCase())
		);
	});

	/** One profile at a time: a second click doesn't make a second one. */
	let creating = $state(false);
	const create: SubmitFunction = () => {
		creating = true;
		return async ({ update }) => {
			await update();
			creating = false;
		};
	};
</script>

{#snippet members(others: Avatar[], size = 'size-8')}
	<span class="flex shrink-0 items-center -space-x-2 max-sm:w-22" aria-hidden="true">
		<UserAvatar {name} picture={data.user?.picture} class="{size} ring-2 ring-background" />
		{#each others as tone, i (i)}
			<span
				class="{size} shrink-0 rounded-full ring-2 ring-background"
				style:background-color="var(--avatar-{tone})"
			></span>
		{/each}
	</span>
{/snippet}

{#snippet problem(from: string)}
	{#if form?.message && form.from === from}
		<p class="text-sm text-destructive" role="alert">{form.message}</p>
	{/if}
{/snippet}

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-10 px-4 py-8 sm:py-12">
			{#if first}
				<header class="space-y-4">
					<span class="flex gap-1.5" aria-hidden="true">
						{#each AVATARS as tone (tone)}
							<span class="size-2 rounded-full" style:background-color="var(--avatar-{tone})"
							></span>
						{/each}
					</span>
					<div class="space-y-2">
						<p class="text-lg text-muted-foreground">{m.profiles.hello(firstName)}</p>
						<h1 class="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
							{m.profiles.start}
						</h1>
					</div>
					<p class="text-muted-foreground">{m.profiles.intro}</p>
				</header>
			{:else}
				<section class="space-y-4">
					<div class="space-y-1">
						<h1 class="text-2xl font-semibold">{m.profiles.title}</h1>
						<p class="text-muted-foreground">{m.profiles.intro}</p>
					</div>
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
											{profile.justYou ? m.profiles.justYou : profile.members.join(', ')}
										</span>
									</span>
									<ChevronRightIcon class="size-4 text-muted-foreground" />
								</a>
							</li>
						{/each}
					</ul>
				</section>
			{/if}

			{#if !data.hasOwn}
				<section class="space-y-5 rounded-3xl border p-5 sm:p-6">
					<div class="flex items-start gap-4">
						<UserAvatar {name} picture={data.user?.picture} class="size-12 text-base" />
						<div class="min-w-0 space-y-1">
							<p class="text-xs font-medium tracking-wide text-muted-foreground uppercase">
								{m.profiles.justYou}
							</p>
							<h2 class="text-lg font-semibold">{m.profiles.own.title}</h2>
							<p class="text-sm text-muted-foreground">{m.profiles.own.about}</p>
						</div>
					</div>
					<form
						method="POST"
						action="?/create"
						use:enhance={create}
						class="flex flex-col gap-2 sm:flex-row"
					>
						<input type="hidden" name="from" value="own" />
						<Input
							name="name"
							value={firstName}
							aria-label={m.profiles.name}
							required
							class="h-10 rounded-full px-4 sm:flex-1"
						/>
						<Button type="submit" class="h-10 px-5" disabled={creating}>
							{m.profiles.own.create}
						</Button>
					</form>
					{@render problem('own')}
				</section>
			{/if}

			<section class="space-y-5">
				<div class="space-y-1">
					<h2 class={first || !data.hasOwn ? 'text-lg font-semibold' : 'font-medium'}>
						{m.profiles.shared.title}
					</h2>
					<p class="text-sm text-muted-foreground">{m.profiles.shared.about}</p>
				</div>

				{#if examples.length}
					<form
						method="POST"
						action="?/create"
						use:enhance={create}
						class="grid gap-3 sm:grid-cols-3"
					>
						{#each examples as example (example.key)}
							<button
								type="submit"
								name="name"
								value={example.name}
								disabled={creating}
								class="group flex items-center gap-4 rounded-2xl border p-4 text-left transition-[border-color,box-shadow] outline-none hover:border-foreground/25 hover:shadow-sm focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/30 disabled:opacity-60 sm:flex-col sm:items-start sm:gap-3"
							>
								{@render members(example.others, 'size-7 sm:size-8')}
								<span class="min-w-0 flex-1 space-y-0.5">
									<span class="block font-medium">{example.name}</span>
									<span class="block text-sm text-muted-foreground">{example.about}</span>
								</span>
								<!-- On a phone, only the plus: the tiles are rows there. -->
								<span
									class="inline-flex items-center gap-1 text-sm font-medium text-muted-foreground group-hover:text-foreground sm:pt-1"
								>
									<PlusIcon class="size-4" />
									<span class="max-sm:sr-only">{m.common.create}</span>
								</span>
							</button>
						{/each}
					</form>
				{/if}

				<form method="POST" action="?/create" use:enhance={create} class="space-y-2">
					<input type="hidden" name="from" value="custom" />
					<label for="new-profile" class="text-sm font-medium">{m.profiles.shared.custom}</label>
					<div class="flex gap-2">
						<Input
							id="new-profile"
							name="name"
							placeholder={m.profiles.namePlaceholder}
							required
							class="h-10 flex-1 rounded-full px-4"
						/>
						<Button type="submit" variant="outline" class="h-10 px-5" disabled={creating}>
							{m.common.create}
						</Button>
					</div>
					{@render problem('custom')}
				</form>

				<p class="text-sm text-muted-foreground">
					<Rich text={m.profiles.shared.addPeople}>
						{#snippet settings()}<span class="font-medium text-foreground">{m.sidebar.people}</span
							>{/snippet}
					</Rich>
					{#if data.user?.isAdmin}
						<Rich text={m.profiles.shared.needsAccount}>
							{#snippet people()}<a
									href={resolve('/admin/people')}
									class="font-medium text-foreground underline-offset-2 hover:underline"
									>{m.userMenu.people}</a
								>{/snippet}
						</Rich>
					{:else}
						{m.profiles.shared.askForAccount}
					{/if}
				</p>
			</section>

			<p class="border-t pt-6 text-sm text-muted-foreground">
				<Rich text={m.profiles.staysHere}>
					{#snippet card()}<a
							href={resolve('/card')}
							class="font-medium text-foreground underline-offset-2 hover:underline"
							>{m.profiles.yourCard}</a
						>{/snippet}
				</Rich>
			</p>
		</div>
	</main>
</div>
