<script lang="ts">
	import { enhance } from '$app/forms';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const done = $derived(form && 'done' in form ? form.done : null);
</script>

<svelte:head>
	<title>Link nolune · nolune</title>
</svelte:head>

<section class="flex flex-col gap-2">
	<h1 class="text-2xl font-semibold">Link nolune</h1>
	{#if done === 'approved' || data.status === 'approved'}
		<p class="text-muted-foreground">
			nolune on your computer is linked to your plan. You can close this page.
		</p>
	{:else if done === 'denied' || data.status === 'denied'}
		<p class="text-muted-foreground">Not linked. Nothing changed on your plan.</p>
	{:else if data.status === 'pending'}
		<p class="text-muted-foreground">
			Check that nolune on your computer shows this code, then link it. It will use your plan for
			chats, pictures and memory search.
		</p>
	{:else}
		<p class="text-muted-foreground">Enter the code nolune shows on your computer.</p>
	{/if}
</section>

{#if data.status === 'pending' && !done}
	<p class="rounded-xl bg-muted py-4 text-center font-mono text-2xl font-semibold tracking-[0.3em]">
		{data.code}
	</p>
	{#if form && 'message' in form}<p class="text-danger text-sm" role="alert">{form.message}</p>{/if}
	<div class="flex gap-3">
		<form method="POST" action="?/approve" class="flex-1" use:enhance>
			<input type="hidden" name="code" value={data.code} />
			<button
				class="w-full rounded-full bg-primary px-4 py-2.5 font-medium text-primary-foreground"
			>
				Link
			</button>
		</form>
		<form method="POST" action="?/deny" class="flex-1" use:enhance>
			<input type="hidden" name="code" value={data.code} />
			<button class="w-full rounded-full border border-border px-4 py-2.5 font-medium">
				Don't link
			</button>
		</form>
	</div>
{:else if !done && data.status !== 'approved' && data.status !== 'denied'}
	<form method="GET" class="flex flex-col gap-3">
		<label class="flex flex-col gap-1.5">
			<span class="text-sm font-medium">Code</span>
			<input
				name="user_code"
				autocomplete="off"
				autocapitalize="characters"
				required
				value={data.code}
				class="rounded-xl border border-border bg-background px-3 py-2.5 font-mono text-lg tracking-[0.2em] uppercase"
			/>
		</label>
		{#if data.status === 'unknown'}
			<p class="text-danger text-sm" role="alert">
				That code isn't one nolune asked for, or it has run out. Check it, or ask nolune for a new
				one.
			</p>
		{/if}
		<button class="rounded-full bg-primary px-4 py-2.5 font-medium text-primary-foreground">
			Continue
		</button>
	</form>
{/if}
