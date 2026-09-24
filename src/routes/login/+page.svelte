<script lang="ts">
	import { enhance } from '$app/forms';

	let { form } = $props();
	let pending = $state(false);
</script>

<main class="flex h-full items-center justify-center p-4">
	<form
		method="POST"
		class="w-full max-w-sm space-y-4 rounded-xl border border-stone-200 bg-white p-6 shadow-sm"
		use:enhance={() => {
			pending = true;
			return async ({ update }) => {
				await update();
				pending = false;
			};
		}}
	>
		<h1 class="text-xl font-semibold">Sign in to btw</h1>
		<label class="block space-y-1 text-sm">
			<span class="text-stone-600">Email</span>
			<input
				name="email"
				type="email"
				autocomplete="username"
				required
				value={form?.email ?? ''}
				class="w-full rounded-md border border-stone-300 px-3 py-2 focus:border-stone-500 focus:outline-none"
			/>
		</label>
		<label class="block space-y-1 text-sm">
			<span class="text-stone-600">Password</span>
			<input
				name="password"
				type="password"
				autocomplete="current-password"
				required
				class="w-full rounded-md border border-stone-300 px-3 py-2 focus:border-stone-500 focus:outline-none"
			/>
		</label>
		{#if form?.message}
			<p class="text-sm text-red-600">{form.message}</p>
		{/if}
		<button
			disabled={pending}
			class="w-full rounded-md bg-stone-900 px-4 py-2 text-sm font-medium text-white hover:bg-stone-700 disabled:opacity-50"
		>
			{pending ? 'Signing in…' : 'Sign in'}
		</button>
	</form>
</main>
