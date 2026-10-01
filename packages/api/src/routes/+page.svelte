<script lang="ts">
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const dollars = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;
	const when = (ms: number | null) =>
		ms === null
			? ''
			: new Date(ms).toLocaleString(undefined, {
					weekday: 'short',
					hour: 'numeric',
					minute: '2-digit'
				});
	const share = (spent: number, limit: number) =>
		limit > 0 ? Math.min(100, Math.round((spent / limit) * 100)) : 0;
</script>

<svelte:head>
	<title>Your plan · nolune</title>
</svelte:head>

<section class="flex flex-col gap-1">
	<h1 class="text-2xl font-semibold">Your plan</h1>
	<p class="text-muted-foreground">{data.email}</p>
</section>

{#if data.usage}
	{@const u = data.usage}
	<dl class="flex flex-col gap-4">
		{#each [{ label: '5 hours', ...u.window }, { label: 'This week', ...u.week }] as limit (limit.label)}
			<div class="flex flex-col gap-1.5">
				<div class="flex justify-between text-sm">
					<dt class="font-medium">{limit.label}</dt>
					<dd class="text-muted-foreground">
						{share(limit.spent, limit.limit)}%{#if limit.resetsAt}
							· again {when(limit.resetsAt)}{/if}
					</dd>
				</div>
				<div class="h-2 overflow-hidden rounded-full bg-muted">
					<div
						class="h-full rounded-full bg-primary"
						style:width="{share(limit.spent, limit.limit)}%"
					></div>
				</div>
			</div>
		{/each}
		<div class="flex justify-between text-sm">
			<dt class="font-medium">Credits left</dt>
			<dd class="text-muted-foreground">
				{dollars(u.credits.plan)}{#if u.credits.extra > 0}
					+ {dollars(u.credits.extra)} extra{/if}
			</dd>
		</div>
	</dl>
{:else}
	<p class="rounded-xl bg-muted p-4 text-sm">
		No plan yet. With one, nolune on your computer gets chats, pictures and memory search with no
		API keys. Plans open soon.
	</p>
{/if}

<form method="POST" action="?/signOut">
	<button class="text-sm text-muted-foreground underline underline-offset-4">Sign out</button>
</form>
