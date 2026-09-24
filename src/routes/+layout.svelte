<script lang="ts">
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import { resolve } from '$app/paths';

	let { data, children } = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>btw</title>
</svelte:head>

<div class="flex h-dvh flex-col bg-stone-50 text-stone-900">
	{#if data.user}
		<header class="flex h-12 shrink-0 items-center gap-4 border-b border-stone-200 bg-white px-4">
			<a href={resolve('/')} class="font-semibold tracking-tight">btw</a>
			<div class="ml-auto flex items-center gap-4 text-sm text-stone-600">
				{#if data.user.isAdmin}
					<a href={resolve('/admin')} class="hover:text-stone-900">Models</a>
				{/if}
				<span>{data.user.name}</span>
				<form method="POST" action="/logout">
					<button class="hover:text-stone-900">Sign out</button>
				</form>
			</div>
		</header>
	{/if}
	<div class="min-h-0 flex-1">
		{@render children()}
	</div>
</div>
