<script lang="ts">
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import type { PageProps } from './$types';

	let { form }: PageProps = $props();
	let sending = $state(false);
</script>

<svelte:head>
	<title>Sign in · nolune</title>
</svelte:head>

<section class="flex flex-col gap-2">
	<h1 class="text-2xl font-semibold">Sign in</h1>
	<p class="text-muted-foreground">
		{#if form?.sent}
			We sent a code to <strong class="text-foreground">{form.email}</strong>. Enter it here.
		{:else}
			Enter your email and we'll send you a code. The first time, that makes your account.
		{/if}
	</p>
</section>

{#if form?.sent}
	<form
		method="POST"
		action="?/verify"
		class="flex flex-col gap-3"
		use:enhance={() => {
			sending = true;
			return async ({ update }) => {
				await update();
				sending = false;
			};
		}}
	>
		<input type="hidden" name="email" value={form.email} />
		<input type="hidden" name="next" value={page.url.searchParams.get('next') ?? '/'} />
		<label class="flex flex-col gap-1.5">
			<span class="text-sm font-medium">Code</span>
			<input
				name="code"
				inputmode="numeric"
				autocomplete="one-time-code"
				maxlength="7"
				required
				class="rounded-xl border border-border bg-background px-3 py-2.5 text-lg tracking-[0.3em]"
			/>
		</label>
		{#if form.message}<p class="text-danger text-sm" role="alert">{form.message}</p>{/if}
		<button
			disabled={sending}
			class="rounded-full bg-primary px-4 py-2.5 font-medium text-primary-foreground disabled:opacity-60"
		>
			Sign in
		</button>
	</form>
	<form method="POST" action="?/send" use:enhance>
		<input type="hidden" name="email" value={form.email} />
		<button class="text-sm text-muted-foreground underline underline-offset-4">
			Send a new code
		</button>
	</form>
{:else}
	<form
		method="POST"
		action="?/send"
		class="flex flex-col gap-3"
		use:enhance={() => {
			sending = true;
			return async ({ update }) => {
				await update({ reset: false });
				sending = false;
			};
		}}
	>
		<label class="flex flex-col gap-1.5">
			<span class="text-sm font-medium">Email</span>
			<input
				name="email"
				type="email"
				autocomplete="email"
				required
				value={form?.email ?? ''}
				class="rounded-xl border border-border bg-background px-3 py-2.5"
			/>
		</label>
		{#if form?.message}<p class="text-danger text-sm" role="alert">{form.message}</p>{/if}
		<button
			disabled={sending}
			class="rounded-full bg-primary px-4 py-2.5 font-medium text-primary-foreground disabled:opacity-60"
		>
			Send me a code
		</button>
	</form>
{/if}
