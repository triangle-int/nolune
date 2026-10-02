<script lang="ts">
	import { tick } from 'svelte';
	import { enhance } from '$app/forms';
	import { Button } from '$lib/components/ui/button';
	import { Switch } from '$lib/components/ui/switch';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { formatTokens } from '$lib/format';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';

	let { data, form } = $props();

	const prefs = getPreferences();
	const { m } = getI18n();

	type Skill = (typeof data.skills)[number];

	const groups = $derived(
		[
			{
				title: m.skills.madeFor(data.profile.name),
				folder: data.folders.profile,
				skills: data.skills.filter((s) => s.scope === 'profile')
			},
			{
				title: m.skills.shared,
				folder: data.folders.global,
				skills: data.skills.filter((s) => s.scope === 'global')
			},
			{
				title: m.skills.builtIn,
				folder: null,
				skills: data.skills.filter((s) => s.scope === 'builtin')
			}
		].filter((g) => g.skills.length)
	);

	const enabled = $derived(data.skills.filter((s) => s.enabled));
	const enabledTokens = $derived(enabled.reduce((n, s) => n + s.tokens, 0));

	// One hidden form carries every change, so a toggle or a whole group is a single request.
	let setForm: HTMLFormElement | undefined = $state();
	let pending = $state<{ names: string[]; enabled: boolean }>({ names: [], enabled: true });

	async function setSkills(skills: Skill[], on: boolean) {
		pending = { names: skills.map((s) => s.name), enabled: on };
		await tick();
		setForm?.requestSubmit();
	}
</script>

<PageHeader>
	<span class="truncate text-lg font-medium">{m.skills.title}</span>
</PageHeader>

<form method="POST" action="?/set" use:enhance bind:this={setForm} class="hidden">
	{#each pending.names as name (name)}
		<input type="hidden" name="name" value={name} />
	{/each}
	<input type="hidden" name="enabled" value={pending.enabled ? 'on' : 'off'} />
</form>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div
		class="mx-auto max-w-2xl space-y-8 px-4 pt-6 pb-[max(1.5rem,env(safe-area-inset-bottom))] sm:py-10"
	>
		<div class="space-y-2">
			<p class="text-muted-foreground">
				{m.skills.intro}
			</p>
			{#if prefs.technical && data.skills.length}
				<p class="text-sm text-muted-foreground">
					{m.skills.summary(enabled.length, data.skills.length, formatTokens(enabledTokens))}
				</p>
			{/if}
		</div>

		{#if form?.message}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		{#each groups as group (group.title)}
			{@const anyOn = group.skills.some((s) => s.enabled)}
			<section class="space-y-3">
				<div class="flex items-end justify-between gap-4">
					<div class="min-w-0 space-y-0.5">
						<h2 class="font-medium">{group.title}</h2>
						{#if group.folder && prefs.technical}
							<p class="truncate font-mono text-xs text-muted-foreground">{group.folder}</p>
						{/if}
					</div>
					{#if group.skills.length > 1}
						<Button
							variant="ghost"
							size="sm"
							class="shrink-0 text-muted-foreground"
							onclick={() => setSkills(group.skills, !anyOn)}
						>
							{anyOn ? m.skills.allOff : m.skills.allOn}
						</Button>
					{/if}
				</div>
				<ul class="overflow-hidden rounded-2xl border">
					{#each group.skills as skill (skill.name)}
						<li class="border-b last:border-b-0">
							<label class="flex cursor-pointer items-start gap-4 px-4 py-3">
								<span class="min-w-0 flex-1 space-y-0.5">
									<span class="flex items-baseline gap-2">
										<span class="truncate font-medium">{skill.name}</span>
										{#if prefs.technical}
											<span class="shrink-0 text-xs text-muted-foreground">
												{m.skills.tokens(formatTokens(skill.tokens))}
											</span>
										{/if}
									</span>
									<span class="line-clamp-2 block text-sm text-muted-foreground">
										{skill.description}
									</span>
								</span>
								<Switch
									class="mt-0.5"
									checked={skill.enabled}
									onCheckedChange={(on) => setSkills([skill], on)}
									aria-label={m.skills.use(skill.name)}
								/>
							</label>
						</li>
					{/each}
				</ul>
			</section>
		{:else}
			<p class="text-muted-foreground">
				{m.skills.empty}
			</p>
		{/each}
	</div>
</div>
