<script lang="ts">
	import { enhance } from '$app/forms';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';

	let { form } = $props();
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
			<h1 class="text-2xl font-medium">Welcome back</h1>
			<p class="text-sm text-muted-foreground">Sign in with the account you were given.</p>
		</div>
		<div class="space-y-3">
			<Input
				name="email"
				type="email"
				autocomplete="username"
				required
				placeholder="Email address"
				aria-label="Email address"
				value={form?.email ?? ''}
				class="h-12 rounded-full px-5 text-base"
			/>
			<Input
				name="password"
				type="password"
				autocomplete="current-password"
				required
				placeholder="Password"
				aria-label="Password"
				class="h-12 rounded-full px-5 text-base"
			/>
		</div>
		{#if form?.message}
			<p class="text-center text-sm text-destructive">{form.message}</p>
		{/if}
		<Button type="submit" disabled={pending} class="h-12 w-full text-base">
			{pending ? 'Signing in…' : 'Continue'}
		</Button>
		<p class="text-center text-xs text-muted-foreground">
			Forgot your password? Ask whoever set up btw to run
			<code class="rounded bg-muted px-1">btw user passwd</code>.
		</p>
	</form>
</main>
