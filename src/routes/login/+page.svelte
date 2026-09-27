<script lang="ts">
	import { enhance } from '$app/forms';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Rich from '$lib/components/Rich.svelte';
	import { getI18n } from '$lib/i18n';

	let { form } = $props();
	const { m } = getI18n();
	let pending = $state(false);
</script>

<main class="flex h-full flex-col items-center justify-center p-4">
	<form
		method="POST"
		class="w-full max-w-sm space-y-6"
		use:enhance={() => {
			pending = true;
			return async ({ update }) => {
				await update();
				pending = false;
			};
		}}
	>
		<div class="space-y-2 text-center">
			<div class="text-4xl font-semibold tracking-tight">btw</div>
			<h1 class="text-2xl font-medium">{m.login.welcome}</h1>
			<p class="text-sm text-muted-foreground">{m.login.hint}</p>
		</div>
		<div class="space-y-3">
			<Input
				name="email"
				type="email"
				autocomplete="username"
				required
				placeholder={m.login.email}
				aria-label={m.login.email}
				value={form?.email ?? ''}
				class="h-12 rounded-full px-5 text-base"
			/>
			<Input
				name="password"
				type="password"
				autocomplete="current-password"
				required
				placeholder={m.login.password}
				aria-label={m.login.password}
				class="h-12 rounded-full px-5 text-base"
			/>
		</div>
		{#if form?.message}
			<p class="text-center text-sm text-destructive">{form.message}</p>
		{/if}
		<Button type="submit" disabled={pending} class="h-12 w-full text-base">
			{pending ? m.login.signingIn : m.common.continue}
		</Button>
		<p class="text-center text-xs text-muted-foreground">
			<Rich text={m.login.forgot}>
				{#snippet command()}<code class="rounded bg-muted px-1">btw user passwd</code>{/snippet}
			</Rich>
		</p>
	</form>
</main>
